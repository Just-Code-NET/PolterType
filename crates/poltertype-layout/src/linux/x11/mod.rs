//! X11 layout switcher via XKB group locking.
//!
//! The backend for sessions with **no desktop environment to ask** —
//! i3, openbox, awesome, a hand-rolled `.xinitrc`. There the X server
//! holds the layout list and switching means locking a different XKB
//! *group*: the up-to-four groups of a keymap are exactly the layouts
//! in `setxkbmap -layout us,ua`.
//!
//! * `current()` → `XkbGetState.locked_group`
//! * `switch_to()` → `XkbLatchLockState { lock_group: true, .. }`
//! * `list_active()` → `_XKB_RULES_NAMES` on the root window, where the
//!   server records the list it was configured with.
//!
//! Probed last: on an X11 session that *does* run a desktop
//! environment, that desktop already owns the layout and keeps a tray
//! indicator in sync, and locking the group underneath it switches the
//! keyboard while leaving the indicator lying.
//!
//! Reference: <https://www.x.org/releases/current/doc/kbproto/xkbproto.html>

#![allow(unused_imports, dead_code)] // Linux-only.

mod consts;
mod switcher;
mod xkb;

pub use consts::*;
pub use switcher::*;
pub use xkb::*;

#[cfg(test)]
mod tests;
