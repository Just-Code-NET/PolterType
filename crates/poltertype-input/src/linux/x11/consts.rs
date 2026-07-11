//! X11 keycode constants and protocol tunables.

use std::time::Duration;

/// X11 keycodes are evdev codes plus a fixed offset of 8.
///
/// The X protocol reserves keycodes 0–7 (0 is "no key"; the rest are
/// historical), so every XKB keymap built by `xkeyboard-config` on
/// Linux shifts the kernel's evdev codes up by 8. This one constant is
/// the whole bridge between the X11 backend and the rest of poltertype:
/// the engine speaks Win SC Set-1 scancodes, which on Linux coincide
/// with evdev codes for every row we buffer (see `wayland::evdev_to_sc1`,
/// which is the identity function for exactly this reason).
///
/// Reference: <https://www.x.org/wiki/Development/Documentation/XKB/>
pub(crate) const EVDEV_OFFSET: u32 = 8;

// ── evdev keycodes we reference by name ─────────────────────────────
// Kernel names from include/uapi/linux/input-event-codes.h.
pub(crate) const EV_BACKSPACE: u32 = 14;
pub(crate) const EV_LEFTCTRL: u32 = 29;
pub(crate) const EV_LEFTSHIFT: u32 = 42;
pub(crate) const EV_RIGHTSHIFT: u32 = 54;
pub(crate) const EV_LEFTALT: u32 = 56;
pub(crate) const EV_CAPSLOCK: u32 = 58;
pub(crate) const EV_RIGHTCTRL: u32 = 97;
pub(crate) const EV_RIGHTALT: u32 = 100;
pub(crate) const EV_LEFTMETA: u32 = 125;
pub(crate) const EV_RIGHTMETA: u32 = 126;

// ── Timing ──────────────────────────────────────────────────────────

/// Sleep between empty `poll_for_event` rounds. Matches the evdev
/// drain loop's idle sleep — small enough to stay imperceptible,
/// large enough that an idle keyboard doesn't spin a core.
pub(crate) const POLL_IDLE: Duration = Duration::from_millis(2);

/// Pacing between the press and release edges of a synthetic key.
///
/// XTest injects straight into the server's event queue, so we do not
/// have the libinput/keyd zero-duration-tap coalescing problem the
/// uinput emitter has to work around. We still pace the stream a
/// little: some toolkits derive auto-repeat and key-held state from
/// event timestamps, and a burst arriving in the same millisecond has
/// been known to confuse them.
pub(crate) const KEY_STEP: Duration = Duration::from_millis(2);

/// How long to let a locked XKB group settle before replaying keys
/// against it. `XkbLatchLockState` is asynchronous: it returns as soon
/// as the server accepts it, but focused clients only see the new
/// group once they process the resulting state-notify. Firing
/// scancodes sooner makes them resolve under the *old* layout — you
/// get the original `lfdfq` back instead of `давай`.
pub(crate) const GROUP_SETTLE: Duration = Duration::from_millis(30);

/// How long to let a remapped scratch keycode settle before tapping
/// it. `ChangeKeyboardMapping` broadcasts a `MappingNotify`, and
/// toolkits re-read the keymap on their own event loop; tapping the
/// key before they do produces the *old* symbol (usually nothing).
pub(crate) const REMAP_SETTLE: Duration = Duration::from_millis(8);
