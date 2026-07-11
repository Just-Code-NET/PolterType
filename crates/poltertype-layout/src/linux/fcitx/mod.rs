//! Fcitx5 layout switcher via `fcitx5-remote`.
//!
//! Fcitx5 thinks in terms of *input methods* (IMs), not "layouts".
//! Each XKB layout is exposed as an IM with a name like
//! `keyboard-us` or `keyboard-ua`. The user-facing CLI is
//! `fcitx5-remote`:
//!
//! * `fcitx5-remote -n` → current IM short name.
//! * `fcitx5-remote -s <name>` → switch.
//!
//! Listing all installed IMs from the CLI alone is unreliable across
//! Fcitx5 versions; for v0.1 we expose `current` + `switch_to` and
//! treat `list_active` as "the current one only" — the engine's
//! layout-mapping DB is the source of truth for what we *try* to
//! switch into.

#![allow(unused_imports, dead_code)] // Linux-only.

mod engines;
mod switcher;

pub use engines::*;
pub use switcher::*;
