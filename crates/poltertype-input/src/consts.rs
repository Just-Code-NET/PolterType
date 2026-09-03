//! Crate-wide constants.

use crate::chords::clipboard_chord;

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
