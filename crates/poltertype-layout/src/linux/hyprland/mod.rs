//! Hyprland layout switcher via `hyprctl`.
//!
//! Hyprland (a tiling Wayland compositor) exposes IPC over a UNIX
//! socket; the canonical user-facing tool is `hyprctl`. Layout config
//! looks like `kb_layout = us,ua` in `hyprland.conf`; switching is by
//! integer index into that list, scoped to a specific keyboard
//! device.
//!
//! Activation: probe `HYPRLAND_INSTANCE_SIGNATURE` — Hyprland sets it
//! on every spawned process.

#![allow(unused_imports, dead_code)] // Linux-only.

mod consts;
mod ipc;
mod parse;
mod switcher;
mod types;

pub use consts::*;
pub use ipc::*;
pub use parse::*;
pub use switcher::*;
pub(crate) use types::*;

#[cfg(test)]
mod tests;
