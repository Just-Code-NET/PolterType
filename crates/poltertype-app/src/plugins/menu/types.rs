//! Plain data behind [`super::state::PluginMenu`]'s runtime submenus.

use tray_icon::menu::Submenu;

/// A submenu whose contents come from the plug-in each time state is
/// read, rather than from the manifest.
pub(super) struct ListMenu {
    /// Index into `extensions`.
    pub(super) ext: usize,
    pub(super) spec: poltertype_core::plugins::TrayList,
    /// The submenu itself, which stays put in the tray menu; only its
    /// contents are replaced. Removing and re-adding the submenu would
    /// move it around the menu as the list filled and emptied.
    pub(super) root: Submenu,
}

/// What a runtime menu entry does when it is clicked: which plug-in,
/// which of its commands, and which row (empty for an action on the whole
/// list).
pub(super) type RowRoute = (usize, String, String);
