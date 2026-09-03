//! Plug-in entries in the tray menu.
//!
//! A plug-in declares menu entries in its manifest; this turns them into
//! real items and remembers which item means which command.
//!
//! Routing by the item's own id — never by label or position — keeps
//! two plug-ins that both call an entry "Settings…" apart, and keeps
//! either from matching one of ours.
//!
//! State is refreshed from the plug-in itself, never from its config
//! file, which holds only what it *starts* as. The live value is shown
//! twice on purpose — a **tick** on the active alternative and a
//! **status line** naming it in words — because a tick is drawn
//! differently by every tray backend, and sometimes not at all.
//!
//! | File | Concern |
//! |---|---|
//! | [`state`] | the struct, its fields, building it from a plug-in list |
//! | [`refresh`] | re-reading plug-in state and redrawing entries |
//! | [`handle`] | routing a menu click to its plug-in |
//! | [`rows`] | parsing a runtime list command's output |
//! | [`types`] | `ListMenu`, one runtime submenu |
//! | [`enums`] | `StateItem`, one state-reflecting entry |

mod enums;
mod handle;
mod refresh;
mod rows;
mod state;
mod types;

pub use rows::parse_rows;
pub use state::PluginMenu;

#[cfg(test)]
mod tests;
