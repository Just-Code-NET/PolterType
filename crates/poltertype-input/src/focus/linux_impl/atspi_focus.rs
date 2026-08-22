//! AT-SPI2 focused-application watcher — the window half of focus
//! tracking on compositors that answer no window query.
//!
//! GNOME and KDE on Wayland expose no "which window is active"
//! interface, so the executable is derived from the a11y bus instead —
//! see [`connection_pid`]. No compositor extension, no user-installed
//! script.
//!
//! **Only applications with a live accessibility bridge are ever
//! seen.** GTK, Qt and Electron-with-a11y answer; a terminal typically
//! does not — and a terminal is precisely where a developer types. An
//! app that never emits also never *un*-focuses the previous one, so
//! the freshest answer can be stale in a way a real window query never
//! is. That is why [`AtspiFocusWatcher::latest`] returns the sample's
//! age and lets the caller decide. Do not describe this as "focus
//! tracking works on GNOME/KDE" — `docs/KNOWN-GAPS.md` carries the
//! full caveat.
//!
//! PRIVACY: this module reads *identity*, never content. Sender names,
//! PIDs and executable paths only — no accessible names, no window
//! titles, no text.

use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tracing::{debug, warn};
use zbus::blocking::connection::Builder;
use zbus::blocking::{Connection, MessageIterator};
use zbus::{MatchRule, Message, message};

use super::atspi_owner::connection_pid;
use super::proc_exe::exe_basename_for_pid;

/// Per-iterator signal queue. Focus changes are rare next to caret
/// motion, but the same burst-shedding logic applies.
const SIGNAL_QUEUE: usize = 16;

#[derive(Debug, Clone)]
pub(crate) struct FocusSample {
    pub(crate) exe: String,
    pub(crate) at: Instant,
}

impl FocusSample {
    pub(crate) fn age(&self) -> Duration {
        self.at.elapsed()
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum AtspiFocusError {
    #[error("session bus unavailable: {0}")]
    SessionBus(zbus::Error),
    #[error("a11y bus address lookup failed: {0}")]
    A11yAddress(zbus::Error),
    #[error("a11y bus connection failed: {0}")]
    A11yConnect(zbus::Error),
    #[error("signal subscription failed: {0}")]
    Subscribe(zbus::Error),
    #[error("watcher thread failed to start: {0}")]
    Spawn(std::io::Error),
}

pub(crate) struct AtspiFocusWatcher {
    latest: Arc<Mutex<Option<FocusSample>>>,
}

impl AtspiFocusWatcher {
    /// Connect, register interest in window activation, start the
    /// thread. Every bus round-trip happens here, on the caller's
    /// thread, so a dead a11y stack surfaces as an error rather than
    /// a silently idle thread.
    pub(crate) fn try_new() -> Result<Self, AtspiFocusError> {
        let session = Connection::session().map_err(AtspiFocusError::SessionBus)?;
        let reply = session
            .call_method(
                Some("org.a11y.Bus"),
                "/org/a11y/bus",
                Some("org.a11y.Bus"),
                "GetAddress",
                &(),
            )
            .map_err(AtspiFocusError::A11yAddress)?;
        let address: String = reply
            .body()
            .deserialize()
            .map_err(AtspiFocusError::A11yAddress)?;
        let conn = Builder::address(address.as_str())
            .map_err(AtspiFocusError::A11yConnect)?
            .build()
            .map_err(AtspiFocusError::A11yConnect)?;

        // Toolkits ask the registry which events have listeners and
        // emit only those, so registering is what makes apps speak.
        // `window:activate` is the one that means "this window is now
        // the active one"; a failure to register is not fatal on its
        // own, because another AT client may already have asked for
        // it and the events would flow regardless.
        if let Err(e) = conn.call_method(
            Some("org.a11y.atspi.Registry"),
            "/org/a11y/atspi/registry",
            Some("org.a11y.atspi.Registry"),
            "RegisterEvent",
            &("window:activate",),
        ) {
            debug!(%e, "AT-SPI focus: RegisterEvent failed; relying on other clients");
        }

        // Same flag the caret watcher raises, same reasoning: without
        // an AT client asserting it, most toolkits keep their bridge
        // dormant and nothing is emitted at all. Session-scoped and
        // never cleared — a real AT arriving later depends on it too.
        if let Err(e) = session.call_method(
            Some("org.a11y.Bus"),
            "/org/a11y/bus",
            Some("org.freedesktop.DBus.Properties"),
            "Set",
            &(
                "org.a11y.Status",
                "IsEnabled",
                zbus::zvariant::Value::from(true),
            ),
        ) {
            debug!(%e, "could not raise org.a11y.Status.IsEnabled; apps may stay silent");
        }

        let rule = MatchRule::builder()
            .msg_type(message::Type::Signal)
            .interface("org.a11y.atspi.Event.Window")
            .map_err(AtspiFocusError::Subscribe)?
            .member("Activate")
            .map_err(AtspiFocusError::Subscribe)?
            .build();
        let messages = MessageIterator::for_match_rule(rule, &conn, Some(SIGNAL_QUEUE))
            .map_err(AtspiFocusError::Subscribe)?;

        let latest = Arc::new(Mutex::new(None));
        let slot = Arc::clone(&latest);
        std::thread::Builder::new()
            .name("poltertype-atspi-focus".into())
            .spawn(move || watch(&conn, messages, &slot))
            .map_err(AtspiFocusError::Spawn)?;
        Ok(Self { latest })
    }

    /// Freshest focus observation, if any has arrived. Cloning a short
    /// string per call — this sits behind the factory's TTL cache.
    pub(crate) fn latest(&self) -> Option<FocusSample> {
        self.latest.lock().clone()
    }
}

/// Blocking signal loop. Ends — with a single `warn` — when the bus
/// dies; the caller degrades to "no focus information", which is
/// where these sessions started.
fn watch(conn: &Connection, messages: MessageIterator, latest: &Mutex<Option<FocusSample>>) {
    for msg in messages {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                warn!(%e, "AT-SPI focus watcher: a11y bus error; focus tracking stops");
                return;
            }
        };
        if let Some(sample) = sample_for_signal(conn, &msg) {
            debug!(exe = %sample.exe, "AT-SPI focus: active application changed");
            *latest.lock() = Some(sample);
        }
    }
    warn!("AT-SPI focus watcher: a11y bus stream ended; focus tracking stops");
}

/// One `window:activate` signal → the activating app's executable.
///
/// The signal body is ignored entirely: it carries accessible names
/// and window titles, which this module must not read. The *sender*
/// is the identity we want.
fn sample_for_signal(conn: &Connection, msg: &Message) -> Option<FocusSample> {
    let header = msg.header();
    let sender = header.sender()?;
    let pid = connection_pid(conn, sender.as_str())?;
    let exe = exe_basename_for_pid(pid)?;
    Some(FocusSample {
        exe,
        at: Instant::now(),
    })
}

#[cfg(test)]
mod tests;
