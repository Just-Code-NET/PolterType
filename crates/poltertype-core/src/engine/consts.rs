//! Timing windows and fixed scancodes the engine matches against.

use std::time::Duration;

/// How long after a paste shortcut we decline to auto-correct. Generous
/// enough to cover a paste replayed as a keystroke burst, short enough
/// that the next genuinely-typed word still gets corrected.
pub const PASTE_GUARD: Duration = Duration::from_millis(1200);

/// SC Set-1 scancode for the `V` key (matches evdev `KEY_V` on Linux).
pub const SC_V: u32 = 0x2F;
/// evdev `KEY_INSERT` — used for the Shift+Insert paste shortcut. (Insert
/// has no plain SC-1 byte; the listener reports the raw evdev code.)
pub const SC_INSERT: u32 = 110;
