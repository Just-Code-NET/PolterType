//! KDE Plasma layout switcher via `qdbus`.
//!
//! Plasma exposes layout state on the session bus at
//! `org.kde.keyboard /Layouts (org.kde.KeyboardLayouts)`. The interface
//! is KWin's `KeyboardLayoutDBusInterface` (`src/keyboard_layout.h`),
//! and since Plasma 5.23 it addresses layouts **by index**:
//!
//! * `getLayoutsList() → a(sss)` — `(shortName, displayName, longName)`
//! * `getLayout() → u` — position in that list
//! * `setLayout(u) → b`
//!
//! Before 5.23 the last two spoke xkb short names (`"us"`) instead;
//! [`switcher::KdeSwitcher`] probes which it is talking to. The struct
//! array needs `qdbus --literal` — see [`parse::layout_short_names`].
//!
//! Activation: `XDG_CURRENT_DESKTOP` contains `"KDE"`.

#![allow(unused_imports, dead_code)] // Linux-only.

mod ipc;
mod parse;
mod switcher;

pub use ipc::*;
pub use switcher::*;

#[cfg(test)]
mod tests;
