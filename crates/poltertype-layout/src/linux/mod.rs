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
//!
//! The floor under that rule is [`names_a_layout`]: whatever a backend
//! answered to "are you running", it is only selected if it can name a
//! layout. Fcitx5 is why — installed and autostarted by Ubuntu's
//! language support, it says yes to both and owns nothing.

#![allow(unused_imports, dead_code)] // Linux-only.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tracing::info;

use crate::{LayoutError, LayoutId, LayoutSwitcher};

pub mod chord;
pub mod cinnamon;
pub mod fcitx;
pub mod gnome;
pub mod hyprland;
pub mod ibus;
pub mod kde;
pub mod shared;
pub mod sway;
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
    let mut mute: Vec<&'static str> = Vec::new();

    macro_rules! probe {
        ($name:literal, $init:expr) => {{
            let built: Option<Box<dyn LayoutSwitcher>> = $init;
            if let Some(s) = built {
                if names_a_layout(s.as_ref()) {
                    return Ok(Box::new(CachedSwitcher::new(s)));
                }
                mute.push($name);
            }
            tried.push($name);
        }};
    }

    probe!("hyprland", hyprland::try_init().map(boxed));
    probe!("kde", kde::try_init().map(boxed));
    // Before gsettings, not after: Cinnamon would pass the gsettings
    // probe and then fail to switch anything (#26).
    probe!("sway", sway::try_init().map(boxed));
    probe!("cinnamon", cinnamon::try_init());
    probe!("gnome", gnome::try_init().map(boxed));
    probe!("ibus", ibus::try_init().map(boxed));
    probe!("fcitx", fcitx::try_init().map(boxed));
    probe!("x11", x11::try_init().map(boxed));

    Err(LayoutError::Unsupported(format!(
        "no Linux layout-switching backend available; probed: {tried:?}, \
         initialised but naming no layout: {mute:?}"
    )))
}

fn boxed<S: LayoutSwitcher + 'static>(s: S) -> Box<dyn LayoutSwitcher> {
    Box::new(s)
}

/// Can this backend name a single layout? If not, it is not the thing
/// driving this session, whatever it answered to "are you running".
///
/// Fcitx5 is the case that named the rule. Ubuntu installs it with
/// language support and autostarts it, so `fcitx5-remote -t 1` exits 0
/// on a desktop where fcitx owns no input method at all — and
/// `fcitx5-remote -n` then answers with an empty line. The backend
/// activated on GNOME, Xfce, MATE, LXQt, Budgie, sway, labwc and every
/// bare WM, reported `active=[LayoutId("")] count=1`, and the layout DB
/// came up with **zero** layouts loaded: a log that reads
/// `layout switcher ready` on an app that can no longer correct
/// anything. Measured across a 17-session sweep, 2026-08-24.
///
/// This is the same rule the KDE backend already applies to itself and
/// the same lesson as [#26](https://github.com/Just-Code-NET/PolterType/issues/26):
/// probe by what a desktop *does*, not by what it ships.
fn names_a_layout(s: &dyn LayoutSwitcher) -> bool {
    match s.list_active() {
        Ok(list) if list.iter().any(|id| !id.as_str().trim().is_empty()) => true,
        Ok(list) => {
            info!(
                backend = s.backend_name(),
                ?list,
                "backend initialised but names no layout — standing down for the next one"
            );
            false
        }
        Err(e) => {
            info!(
                backend = s.backend_name(),
                %e,
                "backend initialised but could not list layouts — standing down for the next one"
            );
            false
        }
    }
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
        "sway" => boxed(sway::try_init()),
        "x11" | "xkb" => boxed(x11::try_init()),
        other => {
            return Err(LayoutError::Unsupported(format!(
                "{BACKEND_ENV}={other:?} names no backend; expected auto, hyprland, kde, \
                 cinnamon, sway, gnome, ibus, fcitx or x11"
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

    /// Straight through, never from the cache: `switch_to` writes the
    /// cache itself, so a cached answer here would confirm our own
    /// write — which is the exact mistake this call exists to catch.
    fn verify_switched(&self, target: &LayoutId) -> Option<bool> {
        self.inner.verify_switched(target)
    }

    fn switch_chord(&self) -> Option<poltertype_types::SwitchChord> {
        self.inner.switch_chord()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A backend that answers whatever the test wants it to.
    struct Fake(Result<Vec<LayoutId>, LayoutError>);

    impl LayoutSwitcher for Fake {
        fn current(&self) -> Result<LayoutId, LayoutError> {
            Ok(LayoutId::new("en-US"))
        }
        fn list_active(&self) -> Result<Vec<LayoutId>, LayoutError> {
            match &self.0 {
                Ok(v) => Ok(v.clone()),
                Err(e) => Err(LayoutError::Unsupported(e.to_string())),
            }
        }
        fn switch_to(&self, _: &LayoutId) -> Result<(), LayoutError> {
            Ok(())
        }
        fn backend_name(&self) -> &'static str {
            "fake"
        }
    }

    fn fake(ids: &[&str]) -> Fake {
        Fake(Ok(ids.iter().map(|s| LayoutId::new(*s)).collect()))
    }

    /// The fcitx5 case: running, answering, and owning nothing. Ubuntu
    /// autostarts it with language support, so this is the default
    /// state on a machine that never configured an input method — and
    /// before this guard it took the layout DB down to zero layouts on
    /// every desktop but KDE and Cinnamon.
    #[test]
    fn a_backend_naming_no_layout_is_not_the_one_driving_the_session() {
        assert!(!names_a_layout(&fake(&[""])), "an empty id names nothing");
        assert!(!names_a_layout(&fake(&[])), "an empty list names nothing");
        assert!(!names_a_layout(&fake(&["  "])), "whitespace names nothing");
        assert!(
            !names_a_layout(&Fake(Err(LayoutError::Unsupported("no".into())))),
            "a backend that cannot be asked cannot be trusted to switch"
        );
    }

    #[test]
    fn a_backend_that_names_one_is_accepted() {
        assert!(names_a_layout(&fake(&["en-US"])));
        assert!(
            names_a_layout(&fake(&["", "ru-RU"])),
            "one real layout among blanks is still a working backend"
        );
    }
}
