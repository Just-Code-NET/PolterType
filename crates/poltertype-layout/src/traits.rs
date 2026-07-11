//! The OS layout-switcher extension point.

use crate::*;
pub use poltertype_types::LayoutId;

pub trait LayoutSwitcher: Send + Sync {
    /// Layout currently effective for the foreground window.
    fn current(&self) -> Result<LayoutId, LayoutError>;

    /// All layouts the system knows about and the user has enabled.
    fn list_active(&self) -> Result<Vec<LayoutId>, LayoutError>;

    /// Switch the foreground window to the given layout. Must be one
    /// of the layouts returned by [`list_active`].
    fn switch_to(&self, id: &LayoutId) -> Result<(), LayoutError>;

    fn backend_name(&self) -> &'static str;
}
