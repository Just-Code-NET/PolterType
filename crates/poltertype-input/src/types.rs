//! Key replay / emission data carried across the traits.

pub use poltertype_types::KeyDirection;

/// A scancode + shift state pair, to be replayed against whatever
/// layout the OS is currently in. Used by the Linux corrector to
/// avoid the Unicode-input compose dance that breaks in terminals
/// and Wayland-native apps.
#[derive(Debug, Clone, Copy)]
pub struct ReplayKey {
    pub scancode: u32,
    pub shift: bool,
}

/// One synthetic keystroke an emitter actually put on the wire.
///
/// Where injected events echo back indistinguishable from real typing
/// — Linux/uinput behind a remapper — the engine collects these via
/// [`KeyEmitter::take_emitted`] and match-and-consumes the echoes,
/// rather than suppressing everything for a fixed window, which eats
/// the first characters typed after a correction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmittedKey {
    pub scancode: u32,
    pub direction: KeyDirection,
}

/// The clipboard chords selection conversion presses into the focused
/// application: `Ctrl+C` / `Ctrl+V`, and `Cmd`-flagged on macOS.
///
/// The split stays in the constants rather than moving into the
/// emitters, and for the reason the emitters make obvious: one that
/// quietly rewrote Ctrl into Cmd would rewrite a user's *explicit*
/// Ctrl hotkey with it, and that is not its call to make. It lives in
/// this crate rather than beside the engine's other constants because
/// this is a crate allowed to know which OS it is on — `poltertype-core`
/// holds no platform conditionals at all, and that is checkable with a
/// grep only while it stays true.
pub const COPY_CHORD: poltertype_types::SwitchChord = clipboard_chord(0x2E);

/// The paste chord that puts the converted selection back. Same split
/// as [`COPY_CHORD`], for the same reason.
pub const PASTE_CHORD: poltertype_types::SwitchChord = clipboard_chord(0x2F);

/// `scancode` is Win SC Set-1, which for `C` and `V` coincides with
/// evdev's `KEY_C` / `KEY_V`.
const fn clipboard_chord(scancode: u32) -> poltertype_types::SwitchChord {
    poltertype_types::SwitchChord {
        scancode,
        ctrl: !cfg!(target_os = "macos"),
        shift: false,
        alt: false,
        meta: cfg!(target_os = "macos"),
    }
}
