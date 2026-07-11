//! X11 layout switcher via XKB group locking.
//!
//! This is the backend for sessions with **no desktop environment to
//! ask** — i3, openbox, awesome, xfce without an input-method daemon, a
//! hand-rolled `.xinitrc`. There the X server itself holds the layout
//! list, and switching means locking a different XKB *group*.
//!
//! An XKB keymap carries up to four groups (`Group::M1`..`M4`), which
//! are exactly the layouts in `setxkbmap -layout us,ua`. The locked
//! group is the one the user is typing in, so:
//!
//! * `current()`   → `XkbGetState.locked_group`
//! * `switch_to()` → `XkbLatchLockState { lock_group: true, .. }`
//! * `list_active()` → the `_XKB_RULES_NAMES` property on the root
//!   window, which is where the server records the layout list it was
//!   configured with (it's what `setxkbmap -query` reads).
//!
//! ## Why this backend is probed last
//!
//! On an X11 session that *does* run a desktop environment, GNOME / KDE
//! / IBus / Fcitx already own the layout and keep a tray indicator in
//! sync with it. Locking the XKB group underneath them switches the
//! keyboard but leaves their indicator lying, so those backends win and
//! this one only picks up what they leave behind.
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
