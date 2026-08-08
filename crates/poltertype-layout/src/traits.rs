//! The OS layout-switcher extension point.

use crate::*;
pub use poltertype_types::{LayoutId, OsKeymap};

pub trait LayoutSwitcher: Send + Sync {
    /// Layout currently effective for the foreground window.
    fn current(&self) -> Result<LayoutId, LayoutError>;

    /// All layouts the system knows about and the user has enabled.
    fn list_active(&self) -> Result<Vec<LayoutId>, LayoutError>;

    /// Switch the foreground window to the given layout. Must be one
    /// of the layouts returned by [`list_active`].
    fn switch_to(&self, id: &LayoutId) -> Result<(), LayoutError>;

    fn backend_name(&self) -> &'static str;

    /// Ask the OS what the user's keyboards actually produce, key by
    /// key — one [`OsKeymap`] per installed keyboard.
    ///
    /// [`current`](Self::current) and [`list_active`](Self::list_active)
    /// can only name a *language*, and a language is not a keyboard:
    /// where a language has several keyboard variants, the bundled
    /// mapping describes one of them and is wrong — silently, and by
    /// most of the alphabet — for the rest. A backend that can answer
    /// this question lets the layout DB correct itself against the
    /// machine it is running on.
    ///
    /// Ordering is significant: the keyboard currently in effect comes
    /// first, because the DB keeps the first entry per language when a
    /// user has installed more than one keyboard for it.
    ///
    /// The default is an empty list — "this backend cannot tell you" —
    /// which leaves the bundled mappings untouched.
    fn describe_keymaps(&self) -> Result<Vec<OsKeymap>, LayoutError> {
        Ok(Vec::new())
    }
}
