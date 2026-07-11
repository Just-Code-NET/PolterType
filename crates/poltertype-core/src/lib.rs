//! poltertype core: settings, layouts, audio, engine.

#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod audio;
pub mod commands;
pub mod data_dir;
pub mod engine;
pub mod layouts;
pub mod settings;
pub mod wordlist_profiles;

pub use commands::{CommandAction, UserCommand};
pub use data_dir::{DataDirError, resolve as resolve_data_dir};
pub use engine::{SwitcherEngine, SwitcherEvent};
pub use layouts::{LayoutDb, LayoutMapping};
pub use settings::{Settings, SettingsStore};
pub use wordlist_profiles::{WordlistProfile, WordlistSettings};
