//! IBus layout switcher via the `ibus` CLI.
//!
//! IBus (Intelligent Input Bus) hosts input methods that can include
//! plain XKB layouts (`xkb:us::eng`, `xkb:ua::ukr`, …). Switching:
//!
//! * `ibus engine` → current engine name.
//! * `ibus engine <name>` → switch.
//! * `ibus list-engine` → all known engines, formatted as
//!   `language - engine_name` blocks.
//!
//! IBus runs on any DE; we probe for `ibus` in PATH.

#![allow(unused_imports, dead_code)] // Linux-only.

mod engines;
mod switcher;

pub use engines::*;
pub use switcher::*;
