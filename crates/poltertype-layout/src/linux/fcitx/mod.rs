//! Fcitx5 layout switcher via `fcitx5-remote`.
//!
//! Fcitx5 thinks in *input methods*, not layouts: each XKB layout is an
//! IM named `keyboard-us`, `keyboard-ua` and so on. `fcitx5-remote -n`
//! reports the current one and `-s <name>` switches.
//!
//! Listing all installed IMs from the CLI is unreliable across Fcitx5
//! versions, so `list_active` reports only the current one — the
//! layout-mapping DB is the source of truth for what we try to switch
//! into.

#![allow(unused_imports, dead_code)] // Linux-only.

mod engines;
mod switcher;

pub use engines::*;
pub use switcher::*;
