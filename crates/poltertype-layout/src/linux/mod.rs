//! Linux layout switcher — shells out to whichever backend the
//! current session uses, in priority order:
//!
//! 1. **Hyprland** (`hyprctl switchxkblayout`) — picked when the
//!    `HYPRLAND_INSTANCE_SIGNATURE` env var is set; the user's
//!    Hyprland config may have `poltertype_layout = us,ua,…` and we cycle by
//!    index.
//! 2. **KDE Plasma** (`qdbus6` / `qdbus` → `org.kde.keyboard`).
//! 3. **GSettings** (`gsettings org.gnome.desktop.input-sources`) —
//!    covers GNOME, Ubuntu Unity 7+, Cinnamon, Budgie, Pantheon
//!    (elementary OS), MATE. `try_init()` here only matches when the
//!    schema is actually installed, so KDE / standalone-Hyprland
//!    sessions fall through.
//! 4. **IBus** (`ibus engine`) — any DE that hosts IBus.
//! 5. **Fcitx5** (`fcitx5-remote`) — any DE that hosts Fcitx.
//!
//! Each backend's `try_init()` does a cheap reachability probe (env
//! var, schema check, or daemon ping). The first that initialises
//! wins. All backends interact with their daemon via the canonical
//! CLI tool shipped with that ecosystem — that's more robust against
//! D-Bus interface drift between distro / DE versions than raw D-Bus
//! calls (and lets us skip the zbus + async-runtime dep entirely).

#![allow(unused_imports, dead_code)] // Linux-only.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::{LayoutError, LayoutId, LayoutSwitcher};

pub mod fcitx;
pub mod gnome;
pub mod hyprland;
pub mod ibus;
pub mod kde;
pub mod shared;
pub mod x11;

pub fn create_switcher() -> Result<Box<dyn LayoutSwitcher>, LayoutError> {
    let mut tried: Vec<&'static str> = Vec::new();

    if let Some(s) = hyprland::try_init() {
        return Ok(Box::new(CachedSwitcher::new(Box::new(s))));
    }
    tried.push("hyprland");

    if let Some(s) = kde::try_init() {
        return Ok(Box::new(CachedSwitcher::new(Box::new(s))));
    }
    tried.push("kde");

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

    Err(LayoutError::Unsupported(format!(
        "no Linux layout-switching backend available; probed: {tried:?}. \
         X11 XkbLockGroup fallback lands in v0.1.x"
    )))
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
