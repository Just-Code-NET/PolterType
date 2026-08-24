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

    /// Did the layout **really** move to `target`?
    ///
    /// `None` means this backend has no reading independent of its own
    /// write, so the question cannot be answered — which is the honest
    /// answer for every backend that switches by setting a key and then
    /// reads that same key back.
    ///
    /// It matters because a switch that silently does not happen is
    /// worse than one that fails: the correction goes ahead, deletes
    /// the user's word and retypes it *identically*. Measured on MATE,
    /// 2026-08-24 — `mate-settings-daemon` restores its own group
    /// within milliseconds of an `XkbLatchLockState`, and the same five
    /// keystrokes came back unchanged.
    fn verify_switched(&self, target: &LayoutId) -> Option<bool> {
        let _ = target;
        None
    }

    /// Ask the OS what the user's keyboards actually produce, key by
    /// key — one [`OsKeymap`] per installed keyboard.
    ///
    /// [`current`](Self::current) and [`list_active`](Self::list_active)
    /// can only name a *language*, and a language is not a keyboard:
    /// where one has several variants the bundled mapping is wrong for
    /// the rest, silently and by most of the alphabet.
    ///
    /// Ordering is significant — the keyboard currently in effect comes
    /// first, because the DB keeps the first entry per language.
    ///
    /// The default empty list means "this backend cannot tell you" and
    /// leaves the bundled mappings untouched.
    fn describe_keymaps(&self) -> Result<Vec<OsKeymap>, LayoutError> {
        Ok(Vec::new())
    }
}
