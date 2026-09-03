//! State behind the Plug-ins pane: one entry per installed extension,
//! holding the values its manifest declared and knowing how to write
//! them back.
//!
//! The pane edits *the plug-in's* config file, written and read by a
//! program we did not write, so two rules apply throughout: only the
//! keys the manifest declared are ever touched, and a write that cannot
//! be made cleanly is reported rather than forced. Everything else in
//! the file, comments included, comes back unchanged — that is
//! [`poltertype_core::plugins::write_setting`]'s whole job.
//!
//! One `impl` block per concern, one file per `impl` block:
//!
//! | File | Concern |
//! |---|---|
//! | [`pane`] | the struct, its fields, construction |
//! | [`types`] / [`enums`] | plain data addressed by, or held for, a box |
//! | [`sections`] | which section is on screen, and what that shows |
//! | [`commands`] | boxes fed by a plug-in command, and their suggestions |
//! | [`records`] | repeating-group rows, and a card's action button |
//! | [`values`] | a plain control's value: typing, settling, writing |
//! | [`arrays`] | a list control's array of ticked members |
//! | [`helpers`] | parsing a command's rows, loading every pane |

// Brought into scope only for `plugin_pane/tests.rs`'s `use super::*;` —
// nothing in this module's own non-test code names them.
#[cfg(test)]
use poltertype_core::plugins::{ControlKind, DiscoveredExtension, PaneControl, SettingValue};
#[cfg(test)]
use std::path::PathBuf;

mod arrays;
mod commands;
mod enums;
mod helpers;
mod pane;
mod records;
mod sections;
mod types;
mod values;

pub use enums::{CommandOutput, Typing};
pub use helpers::load_all;
pub use pane::PluginPane;
pub use types::Slot;

#[cfg(test)]
mod tests;
