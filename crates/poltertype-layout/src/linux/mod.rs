//! Linux layout switcher — shells out to whichever backend the
//! current session uses, in priority order:
//!
//! 1. **Hyprland** (`hyprctl switchxkblayout`) — picked when the
//!    `HYPRLAND_INSTANCE_SIGNATURE` env var is set; the user's
//!    Hyprland config may have `kb_layout = us,ua,…` and we cycle by
//!    index.
//! 2. **KDE Plasma** (`qdbus6` / `qdbus` → `org.kde.keyboard`).
//! 3. **Cinnamon** (`gdbus` → `org.Cinnamon`, or XKB groups on 6.4 and
//!    older). Ahead of GSettings because Cinnamon ships that schema,
//!    populates it, and never reads it — see [`cinnamon`].
//! 4. **GSettings** (`gsettings org.gnome.desktop.input-sources`) —
//!    GNOME, Ubuntu Unity 7+, Budgie, Pantheon (elementary OS).
//!    `try_init()` here only matches when the schema is actually
//!    installed *and populated*.
//! 5. **IBus** (`ibus engine`) — any DE that hosts IBus and lets it
//!    own the layout.
//! 6. **Fcitx5** (`fcitx5-remote`) — any DE that hosts Fcitx.
//! 7. **X11 XKB** (`XkbLatchLockState` via `x11rb`) — the bare-WM
//!    fallback (i3, openbox, plain `.xinitrc`), where no desktop
//!    environment owns the layout and the X server itself holds it.
//!    Last on purpose: where a DE *is* present it keeps a tray
//!    indicator in sync with the layout, and locking the XKB group
//!    underneath it would switch the keyboard while leaving that
//!    indicator lying. Cinnamon 6.4 is the exception that proves the
//!    rule — there the indicator is *driven by* the XKB group, which
//!    is why that case routes here deliberately rather than by
//!    falling through.
//!
//! Each backend's `try_init()` does a cheap reachability probe (env
//! var, schema check, or daemon ping). The first that initialises
//! wins. Setting [`BACKEND_ENV`] skips the probe entirely and pins one
//! backend — the escape hatch for an input stack we guessed wrong
//! about, and the first thing to ask a bug reporter to try.
//!
//! The DE backends interact with their daemon via the canonical CLI
//! tool shipped with that ecosystem — that's more robust against
//! D-Bus interface drift between distro / DE versions than raw D-Bus
//! calls (and lets us skip the zbus + async-runtime dep entirely).
//! X11 is the exception: it speaks the protocol directly, because
//! there is no daemon to ask and `setxkbmap` cannot switch a group —
//! it can only re-install the whole layout list.
//!
//! ## Probing by what a desktop *does*, not by what it ships
//!
//! Issue [#26](https://github.com/Just-Code-NET/PolterType/issues/26)
//! is the cautionary tale for everything above: a schema being
//! installed, populated and writable says nothing about whether
//! anyone reads it. A probe that only checks reachability can hand
//! the session to a backend whose every write is a no-op, and the
//! failure is silent in the worst way — we read our own write back
//! and conclude the layout changed. Where a backend can be asked
//! something only the real owner of the layout could answer (does
//! this method exist? does this desktop drive this schema?), ask
//! that instead.

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

/// Pins one backend instead of probing for it: `cinnamon`, `ibus`,
/// `gnome`, `kde`, `hyprland`, `fcitx`, `x11` — or `auto` (the
/// default) to probe.
///
/// Unset, empty or `auto` changes nothing. A name we don't know, or a
/// backend that cannot initialise on this machine, is an error rather
/// than a quiet fall-through to the probe: someone who pinned a
/// backend wants to hear that it didn't happen, and "we picked a
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
/// Every Linux backend answers `current()` / `list_active()` by
/// talking to an external process or socket. The engine calls
/// `current()` on (nearly) every keystroke to resolve the produced
/// character, and 3-4 more times per completed word — uncached, that
/// used to spawn a `hyprctl` subprocess per keystroke, which both
/// burned CPU and stretched the window between "user typed the word
/// boundary" and "our backspaces reach the screen" to 100 ms+. Keys
/// the user typed inside that window got eaten by the correction —
/// the "first letter of the word stays behind" bug.
///
/// `current()` is cached for a short TTL (manual layout switches by
/// the user must surface quickly); `list_active()` for a longer one
/// (the set changes only when the user edits compositor config). A
/// successful `switch_to()` updates the `current` cache immediately,
/// so the engine sees the new layout with no round-trip — important
/// for classifying keystrokes that race the correction.
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
