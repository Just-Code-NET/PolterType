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

    /// Do we currently believe any *held* modifier is down? Caps Lock
    /// is excluded: it is a latch we toggle on the press edge, not a
    /// key whose held-ness the server could contradict.
    pub(crate) fn any_held(&self) -> bool {
        self.shift || self.control || self.alt || self.meta
    }

    /// Reconcile the latched flags against the server's own view of
    /// which keys are physically down, as `XQueryKeymap` reports it.
    /// Returns whether anything actually changed.
    ///
    /// Tracking modifiers off press/release edges is only correct while
    /// we see every edge, and we do not: any client holding an active
    /// keyboard grab stops XInput2 raw events reaching us for as long
    /// as it holds one. A modifier pressed just before such a grab and
    /// released inside it stays latched here forever — and because
    /// `Modifiers::is_command()` is true whenever Ctrl/Alt/Meta is
    /// held, the engine then reads every later keystroke as a shortcut
    /// and abandons the word buffer. The app goes quiet with no error
    /// and stays quiet until it is restarted (issue #26).
    ///
    /// Caps Lock is deliberately untouched: `XQueryKeymap` reports the
    /// physical key, not the lock state, so folding it in here would
    /// clear the latch every time the user let go of the key.
    pub(crate) fn resync(&mut self, keys: &[u8; 32]) -> bool {
        let held = |left: u32, right: u32| {
            [left, right]
                .into_iter()
                .any(|evdev| evdev_to_x11(evdev).is_some_and(|code| keycode_is_down(keys, code)))
        };
        let before = (self.shift, self.control, self.alt, self.meta);
        self.shift = held(EV_LEFTSHIFT, EV_RIGHTSHIFT);
        self.control = held(EV_LEFTCTRL, EV_RIGHTCTRL);
        self.alt = held(EV_LEFTALT, EV_RIGHTALT);
        self.meta = held(EV_LEFTMETA, EV_RIGHTMETA);
        before != (self.shift, self.control, self.alt, self.meta)
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
