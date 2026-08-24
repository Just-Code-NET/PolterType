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
    /// Caps Lock **latched**, as the server reports it — never a count
    /// of how often the key was pressed. `caps:escape`,
    /// `grp:caps_toggle` and friends give the key another job
    /// entirely, and then it latches nothing; counting edges left the
    /// engine convinced Caps Lock was on for the rest of the session.
    caps: bool,
    /// A Caps Lock edge went by and the latch has not been re-read
    /// from the server since.
    caps_stale: bool,
}

impl ModState {
    pub(crate) fn press(&mut self, evdev: u32) {
        match evdev {
            EV_LEFTSHIFT | EV_RIGHTSHIFT => self.shift = true,
            EV_LEFTCTRL | EV_RIGHTCTRL => self.control = true,
            EV_LEFTALT | EV_RIGHTALT => self.alt = true,
            EV_LEFTMETA | EV_RIGHTMETA => self.meta = true,
            // Whether this press latched anything is the server's to
            // answer — see `caps` and `events::resync_caps`.
            EV_CAPSLOCK => self.caps_stale = true,
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
    /// is excluded: it is a latch, not a key whose held-ness
    /// `XQueryKeymap` could contradict.
    pub(crate) fn any_held(&self) -> bool {
        self.shift || self.control || self.alt || self.meta
    }

    /// Has a Caps Lock edge gone by since the latch was last read?
    /// Clears the flag — the caller is expected to ask the server.
    pub(crate) fn take_caps_stale(&mut self) -> bool {
        std::mem::take(&mut self.caps_stale)
    }

    /// Record the latch as the server reports it.
    pub(crate) fn set_caps(&mut self, caps: bool) {
        self.caps = caps;
    }

    /// Reconcile the latched flags against the server's own view of
    /// which keys are physically down, as `XQueryKeymap` reports it.
    /// Returns whether anything changed.
    ///
    /// Edges go missing behind another client's keyboard grab; the
    /// consequences are in `events::resync_modifiers`.
    ///
    /// Caps Lock is deliberately untouched: `XQueryKeymap` reports the
    /// physical key, not the lock state, so folding it in would clear
    /// the latch every time the user let go of the key.
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

    /// The modifier set as the engine wants it. `shift` is the
    /// physical key and nothing else: it is what a replay presses, and
    /// xkb applies the lock on top of it a second time.
    pub(crate) fn snapshot(&self) -> Modifiers {
        Modifiers {
            shift: self.shift,
            control: self.control,
            alt: self.alt,
            meta: self.meta,
            caps: self.caps,
        }
    }
}
