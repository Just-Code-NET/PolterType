//! Cinnamon 6.4 and older, driven through the X11 XKB backend.

use super::*;
use crate::linux::x11;
use crate::{LayoutError, LayoutId, LayoutSwitcher};

/// Cinnamon 6.4 and older: the X11 backend, chosen deliberately rather
/// than reached as a fallback. Wrapping it only changes the name it
/// reports, and that name is the whole point — see [`XKB_BACKEND_NAME`].
pub struct CinnamonXkbSwitcher(pub(crate) x11::X11Switcher);

impl LayoutSwitcher for CinnamonXkbSwitcher {
    fn current(&self) -> Result<LayoutId, LayoutError> {
        self.0.current()
    }

    fn list_active(&self) -> Result<Vec<LayoutId>, LayoutError> {
        self.0.list_active()
    }

    fn switch_to(&self, id: &LayoutId) -> Result<(), LayoutError> {
        self.0.switch_to(id)
    }

    fn backend_name(&self) -> &'static str {
        XKB_BACKEND_NAME
    }
}
