//! TTL cache in front of a Linux backend.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::{LayoutError, LayoutId, LayoutSwitcher};

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
pub(crate) struct CachedSwitcher {
    inner: Box<dyn LayoutSwitcher>,
    current: Mutex<Option<(LayoutId, Instant)>>,
    list: Mutex<Option<(Vec<LayoutId>, Instant)>>,
}

const CURRENT_TTL: Duration = Duration::from_millis(200);
const LIST_TTL: Duration = Duration::from_secs(2);

impl CachedSwitcher {
    pub(crate) fn new(inner: Box<dyn LayoutSwitcher>) -> Self {
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
