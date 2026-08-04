//! Plug-ins, from this app's side.
//!
//! [`poltertype_core::plugins`] decides what a plug-in *is* — the
//! manifest, what may be installed, and how one is found. This module
//! is what the tray does with the answer: start the long-running half,
//! notice when it dies, run a command behind a menu entry, and stop
//! everything on the way out.
//!
//! Nothing here loads third-party code. A plug-in is a process.

mod menu;
mod supervisor;

pub use menu::PluginMenu;
pub use supervisor::{Supervisor, run_command};
