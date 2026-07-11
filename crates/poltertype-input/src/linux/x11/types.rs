//! State carried across the X11 listener and emitter.

use super::codes::*;
use super::consts::*;
use crate::Modifiers;
use x11rb::protocol::xproto::Window;
use x11rb::rust_connection::RustConnection;

/// An open X11 connection plus the root window every request needs.
pub(crate) struct X11Conn {
    pub(crate) conn: RustConnection,
    pub(crate) root: Window,
}

/// Modifier state, tracked by watching press/release edges.
///
/// XInput2 raw events deliberately carry no modifier state — they are
/// "raw" precisely in the sense of being pre-XKB, reporting the
/// hardware event before the server resolves it against the keymap. So
/// we track the modifiers ourselves off the same event stream, exactly
/// as the evdev backend does (see `wayland::update_modifiers`).
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ModState {
    shift: bool,
    control: bool,
    alt: bool,
    meta: bool,
    caps: bool,
}

impl ModState {
    pub(crate) fn press(&mut self, evdev: u32) {
        match evdev {
            EV_LEFTSHIFT | EV_RIGHTSHIFT => self.shift = true,
            EV_LEFTCTRL | EV_RIGHTCTRL => self.control = true,
            EV_LEFTALT | EV_RIGHTALT => self.alt = true,
            EV_LEFTMETA | EV_RIGHTMETA => self.meta = true,
            // Caps Lock toggles on the press edge, not the release.
            EV_CAPSLOCK => self.caps = !self.caps,
            _ => {}
        }
    }

    pub(crate) fn release(&mut self, evdev: u32) {
        match evdev {
            EV_LEFTSHIFT | EV_RIGHTSHIFT => self.shift = false,
            EV_LEFTCTRL | EV_RIGHTCTRL => self.control = false,
            EV_LEFTALT | EV_RIGHTALT => self.alt = false,
            EV_LEFTMETA | EV_RIGHTMETA => self.meta = false,
            _ => {}
        }
    }

    /// The modifier set as the engine wants it: `shift` already folded
    /// together with Caps Lock, because downstream all that matters is
    /// whether the keystroke produced an uppercase glyph (`Lfdfq` →
    /// `Давай`, not `давай`).
    pub(crate) fn snapshot(&self) -> Modifiers {
        Modifiers {
            shift: self.shift ^ self.caps,
            control: self.control,
            alt: self.alt,
            meta: self.meta,
        }
    }
}
