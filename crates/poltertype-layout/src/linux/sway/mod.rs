//! sway layout switcher via `swaymsg`.
//!
//! sway keeps its keyboard configuration itself and reads no settings
//! schema, so before this backend existed a sway session had no way to
//! switch at all — the gsettings backend would claim it, write a key
//! nobody acts on, and the correction retyped the word unchanged.
//!
//! The IPC gives all three things a backend needs and gives them
//! honestly: `xkb_layout_names` is the configured list,
//! `xkb_active_layout_index` is what sway is applying right now, and
//! `xkb_switch_layout <n>` moves it. The read is sway's own state
//! rather than a copy of our write, which is what lets
//! [`LayoutSwitcher::verify_switched`] mean something here.
//!
//! Its cousins are deliberately absent. niri and river expose the same
//! kind of CLI (`niri msg action switch-layout`,
//! `riverctl keyboard-layout`) but neither is packaged for the distro
//! the desktop matrix runs, so a backend for them could not be run even
//! once before shipping — and this file exists because of what shipping
//! unverified layout backends cost. See `docs/KNOWN-GAPS.md`.

#![allow(unused_imports, dead_code)] // Linux-only.

mod parse;
mod switcher;

pub(crate) use parse::*;
pub use switcher::*;
