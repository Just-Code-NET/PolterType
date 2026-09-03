//! Linux layout switcher — shells out to whichever backend the session
//! uses, probed in priority order: Hyprland, KDE Plasma, Cinnamon,
//! GSettings (GNOME and friends), IBus, Fcitx5, and X11 XKB last.
//!
//! X11 is last on purpose: where a desktop environment is present it
//! keeps a tray indicator in sync, and locking the XKB group underneath
//! would switch the keyboard while leaving that indicator lying.
//! Cinnamon 6.4 is the exception that proves the rule — there the
//! indicator *is* driven by the XKB group, so that case routes here
//! deliberately rather than by falling through. Cinnamon sits ahead of
//! GSettings because it ships that schema, populates it, and never
//! reads it.
//!
//! Desktop backends drive their daemon through the canonical CLI tool
//! of that ecosystem, which survives D-Bus interface drift between
//! distro versions and costs no zbus/async dependency. X11 speaks the
//! protocol directly: there is no daemon to ask, and `setxkbmap` cannot
//! switch a group, only re-install the whole list.
//!
//! **Probe by what a desktop *does*, not by what it ships.** A backend
//! whose every write is a no-op fails silently in the worst way: we read
//! our own write back and conclude the layout changed. Where a backend
//! can be asked something only the real owner of the layout could
//! answer, ask that instead.
//! ([#26](https://github.com/Just-Code-NET/PolterType/issues/26).)
//!
//! The floor under that rule is [`probe::names_a_layout`]: whatever a
//! backend answered to "are you running", it is only selected if it can
//! name a layout. Fcitx5 is why — installed and autostarted by Ubuntu's
//! language support, it says yes to both and owns nothing.

#![allow(unused_imports, dead_code)] // Linux-only.

pub mod chord;
pub mod cinnamon;
pub mod fcitx;
pub mod gnome;
pub mod hyprland;
pub mod ibus;
pub mod kde;
pub mod shared;
pub mod sway;
pub mod x11;

mod cached_switcher;
mod consts;
mod probe;

pub use consts::BACKEND_ENV;
pub use probe::create_switcher;

#[cfg(test)]
mod tests;
