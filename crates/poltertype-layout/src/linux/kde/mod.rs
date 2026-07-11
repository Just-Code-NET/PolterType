//! KDE Plasma layout switcher via `qdbus`.
//!
//! KDE Plasma 5+ exposes layout state on the session bus at
//! `org.kde.keyboard /Layouts (org.kde.KeyboardLayouts)`:
//!
//! * `getLayoutsList() → as`
//! * `getLayout() → s` (BCP-47-style short name like `"us"` / `"ua"`)
//! * `setLayout(s) → b`
//!
//! Plasma 6 ships `qdbus6`; older systems have `qdbus` (Qt5). We try
//! both. Activation: `XDG_CURRENT_DESKTOP` contains `"KDE"` or
//! `KDE_FULL_SESSION=true`.

#![allow(unused_imports, dead_code)] // Linux-only.

mod ipc;
mod switcher;

pub use ipc::*;
pub use switcher::*;
