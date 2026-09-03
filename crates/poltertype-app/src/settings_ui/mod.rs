//! Iced-based Settings window, run as its own process
//! (`poltertype --settings`) because the tray already owns the
//! platform main thread on macOS. The two share nothing but
//! `config.toml`; the named pane is the whole protocol between them.
//!
//! See `docs/ARCHITECTURE.md` § Settings UI for the reasoning and for
//! the three ordering constraints this module has to respect.

mod consts;
mod enums;

pub use enums::Pane;
mod helpers;
mod plugin_pane;
mod run;
mod state;
// Also the tray's: a `mono` icon has to be drawn in the polarity the
// desktop prefers, and this is the probe that knows it.
pub(crate) mod system_theme;
mod theme;
mod types;
mod update;
mod view;
mod view_plugins;
mod view_setup;

pub use run::{run, run_on};

#[cfg(test)]
mod tests;
