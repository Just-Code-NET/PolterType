//! Plain data an entry needs to redraw itself on the next refresh.

use tray_icon::menu::{CheckMenuItem, MenuItem};

/// A menu entry that mirrors plug-in state, and how to redraw it.
pub(super) enum StateItem {
    /// Ticked when the reported value matches.
    Check {
        item: CheckMenuItem,
        /// Kept whole: the label carries a glyph that has to be
        /// re-rendered whenever the live alternative changes.
        spec: poltertype_core::plugins::TrayItem,
    },
    /// A disabled line naming the current value.
    Status {
        item: MenuItem,
        /// Kept whole rather than as a rendered string: the label is a
        /// template and has to be re-rendered on every refresh.
        spec: poltertype_core::plugins::TrayItem,
    },
}

/// One line of a runtime submenu, before it is a menu item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ListEntry {
    /// A line to read: disabled, so it answers a hover with nothing.
    Note(String),
    /// A line to click. An empty `row` means the whole list.
    Action {
        label: String,
        command: String,
        row: String,
    },
    Separator,
}
