//! macOS layout switcher (Phase 5 fills this in).
//!
//! Will use `TISCreateInputSourceList` + `TISSelectInputSource`.

use crate::{LayoutError, LayoutId, LayoutSwitcher};

pub struct MacosLayoutSwitcher;

impl MacosLayoutSwitcher {
    pub fn new() -> Self {
        Self
    }
}

impl LayoutSwitcher for MacosLayoutSwitcher {
    fn current(&self) -> Result<LayoutId, LayoutError> {
        Err(LayoutError::Unsupported(
            "macOS layout switcher not implemented yet (Phase 5)".into(),
        ))
    }
    fn list_active(&self) -> Result<Vec<LayoutId>, LayoutError> {
        Err(LayoutError::Unsupported(
            "macOS layout switcher not implemented yet (Phase 5)".into(),
        ))
    }
    fn switch_to(&self, _id: &LayoutId) -> Result<(), LayoutError> {
        Err(LayoutError::Unsupported(
            "macOS layout switcher not implemented yet (Phase 5)".into(),
        ))
    }
    fn backend_name(&self) -> &'static str {
        "macos-tis-stub"
    }
}
