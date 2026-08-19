//! Linux layout switcher — shells out to whichever backend the session
//! uses, probed in priority order: Hyprland, KDE Plasma, Cinnamon,
//! GSettings (GNOME and friends), IBus, Fcitx5, and X11 XKB last.
//!
//! X11 is last on purpose: where a desktop environment is present it
//! keeps a tray indicator in sync, and locking the XKB group underneath
//! would switch the keyboard while leaving that indicator lying.
//! Cinnamon 6.4 is the exception that proves the rule — there the
//! indicator *is* driven by the XKB group, so that case routes here
//! deliberately rather than by falling through. Cinnamon sits ahead of
//! GSettings because it ships that schema, populates it, and never
//! reads it.
//!
//! Desktop backends drive their daemon through the canonical CLI tool
//! of that ecosystem, which survives D-Bus interface drift between
//! distro versions and costs no zbus/async dependency. X11 speaks the
//! protocol directly: there is no daemon to ask, and `setxkbmap` cannot
//! switch a group, only re-install the whole list.
//!
//! **Probe by what a desktop *does*, not by what it ships.** A backend
//! whose every write is a no-op fails silently in the worst way: we read
//! our own write back and conclude the layout changed. Where a backend
//! can be asked something only the real owner of the layout could
//! answer, ask that instead.
//! ([#26](https://github.com/Just-Code-NET/PolterType/issues/26).)

#![allow(unused_imports, dead_code)] // Linux-only.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tracing::info;

use crate::{LayoutError, LayoutId, LayoutSwitcher};

pub mod cinnamon;
pub mod fcitx;
pub mod gnome;
pub mod hyprland;
pub mod ibus;
pub mod kde;
pub mod shared;
pub mod x11;

/// Pins one backend instead of probing: `cinnamon`, `ibus`, `gnome`,
/// `kde`, `hyprland`, `fcitx`, `x11`, or `auto`. The first thing to ask
/// a bug reporter to try.
///
/// An unknown name, or a backend that cannot initialise here, is an
/// error rather than a quiet fall-through to the probe: "we picked a
/// different one and said nothing" is the exact failure this variable
/// exists to diagnose.
pub const BACKEND_ENV: &str = "POLTERTYPE_LAYOUT_BACKEND";

pub fn create_switcher() -> Result<Box<dyn LayoutSwitcher>, LayoutError> {
    if let Some(name) = pinned_backend_name() {
        return create_pinned_switcher(&name);
    }

    let mut tried: Vec<&'static str> = Vec::new();

    if let Some(s) = hyprland::try_init() {
        return Ok(Box::new(CachedSwitcher::new(Box::new(s))));
    }
    tried.push("hyprland");

    if let Some(s) = kde::try_init() {
        return Ok(Box::new(CachedSwitcher::new(Box::new(s))));
    }
    tried.push("kde");

    // Before gsettings, not after: Cinnamon would pass the gsettings
    // probe and then fail to switch anything (#26).
    if let Some(s) = cinnamon::try_init() {
        return Ok(Box::new(CachedSwitcher::new(s)));
    }
    tried.push("cinnamon");

    if let Some(s) = gnome::try_init() {
        return Ok(Box::new(CachedSwitcher::new(Box::new(s))));
    }
    tried.push("gnome");

    if let Some(s) = ibus::try_init() {
        return Ok(Box::new(CachedSwitcher::new(Box::new(s))));
    }
    tried.push("ibus");

    if let Some(s) = fcitx::try_init() {
        return Ok(Box::new(CachedSwitcher::new(Box::new(s))));
    }
    tried.push("fcitx");

    if let Some(s) = x11::try_init() {
        return Ok(Box::new(CachedSwitcher::new(Box::new(s))));
    }
    tried.push("x11");

    Err(LayoutError::Unsupported(format!(
        "no Linux layout-switching backend available; probed: {tried:?}"
    )))
}

/// The backend name [`BACKEND_ENV`] asks for, normalised — `None` when
/// the variable is unset, blank or `auto`.
fn pinned_backend_name() -> Option<String> {
    let raw = std::env::var(BACKEND_ENV).ok()?;
    let name = raw.trim().to_ascii_lowercase();
    (!name.is_empty() && name != "auto").then_some(name)
}

fn create_pinned_switcher(name: &str) -> Result<Box<dyn LayoutSwitcher>, LayoutError> {
    fn boxed<S: LayoutSwitcher + 'static>(s: Option<S>) -> Option<Box<dyn LayoutSwitcher>> {
        s.map(|s| Box::new(s) as Box<dyn LayoutSwitcher>)
    }

    let built = match name {
        "hyprland" => boxed(hyprland::try_init()),
        "kde" | "plasma" => boxed(kde::try_init()),
        "cinnamon" => cinnamon::init_without_session_check(),
        "gnome" | "gsettings" => boxed(gnome::init_without_desktop_check()),
        "ibus" => boxed(ibus::try_init()),
        "fcitx" | "fcitx5" => boxed(fcitx::try_init()),
        "x11" | "xkb" => boxed(x11::try_init()),
        other => {
            return Err(LayoutError::Unsupported(format!(
                "{BACKEND_ENV}={other:?} names no backend; expected auto, hyprland, kde, \
                 cinnamon, gnome, ibus, fcitx or x11"
            )));
        }
    };

    match built {
        Some(s) => {
            info!(backend = name, "layout backend pinned by {BACKEND_ENV}");
            Ok(Box::new(CachedSwitcher::new(s)))
        }
        None => Err(LayoutError::Unsupported(format!(
            "{BACKEND_ENV}={name} was asked for, but that backend does not initialise here"
        ))),
    }
}

/// TTL cache in front of a Linux backend.
///
/// Every backend answers `current()` by talking to an external process
/// or socket, and the engine asks on nearly every keystroke. Uncached,
/// that spawned a `hyprctl` per keystroke and stretched the gap between
/// the word boundary and our backspaces past 100 ms — keys typed inside
/// that window were eaten, the "first letter stays behind" bug.
///
/// `current()` gets a short TTL so manual switches surface quickly;
/// `list_active()` a longer one, since it changes only on a config
/// edit. A successful `switch_to()` updates the cache immediately, so
/// keystrokes racing the correction are classified against the new
/// layout with no round-trip.
struct CachedSwitcher {
    inner: Box<dyn LayoutSwitcher>,
    current: Mutex<Option<(LayoutId, Instant)>>,
    list: Mutex<Option<(Vec<LayoutId>, Instant)>>,
}

const CURRENT_TTL: Duration = Duration::from_millis(200);
const LIST_TTL: Duration = Duration::from_secs(2);

impl CachedSwitcher {
    fn new(inner: Box<dyn LayoutSwitcher>) -> Self {
        Self {
            inner,
            current: Mutex::new(None),
            list: Mutex::new(None),
        }
    }
}

impl LayoutSwitcher for CachedSwitcher {
    fn current(&self) -> Result<LayoutId, LayoutError> {
        let mut g = self.current.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((id, at)) = g.as_ref() {
            if at.elapsed() < CURRENT_TTL {
                return Ok(id.clone());
            }
        }
        let fresh = self.inner.current()?;
        *g = Some((fresh.clone(), Instant::now()));
        Ok(fresh)
    }

    fn list_active(&self) -> Result<Vec<LayoutId>, LayoutError> {
        let mut g = self.list.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((list, at)) = g.as_ref() {
            if at.elapsed() < LIST_TTL {
                return Ok(list.clone());
            }
        }
        let fresh = self.inner.list_active()?;
        *g = Some((fresh.clone(), Instant::now()));
        Ok(fresh)
    }

    fn switch_to(&self, id: &LayoutId) -> Result<(), LayoutError> {
        self.inner.switch_to(id)?;
        *self.current.lock().unwrap_or_else(|p| p.into_inner()) =
            Some((id.clone(), Instant::now()));
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        self.inner.backend_name()
    }
}
