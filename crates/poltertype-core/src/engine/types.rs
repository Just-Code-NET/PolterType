//! Plain data carried around by the engine: hotkey chords, the
//! stashed last word, and the correction-window drain summary.

use poltertype_input::KeyEvent;
use poltertype_layout::LayoutId;

/// A resolved hotkey chord matched against the raw key stream.
///
/// `scancode` is Win SC Set-1 (the layout-independent identifier the
/// listener already produces — see [`poltertype_types::KeyEvent::scancode`]).
/// Modifier fields are matched exactly: extra held modifiers do *not*
/// match, so `Ctrl+Shift+Space` never fires on `Ctrl+Shift+Alt+Space`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
    pub scancode: u32,
}

/// The two engine hotkeys, resolved to key-stream chords. `None` means
/// "not bound on this backend".
#[derive(Debug, Clone, Copy, Default)]
pub struct KeystreamHotkeys {
    pub pause: Option<Chord>,
    pub switch_last: Option<Chord>,
}

/// Per-chord rising-edge tracking. evdev reports autorepeat as repeated
/// presses, so we latch on the first press and only re-arm on release —
/// one fire per physical keypress, no matter how long it's held.
#[derive(Default)]
pub struct ChordState {
    pub pause_key_down: bool,
    pub switch_key_down: bool,
}

#[derive(Debug, Clone)]
pub struct LastWord {
    pub keys: Vec<poltertype_types::WordKey>,
    pub rendered: String,
    pub layout: LayoutId,
    /// The boundary character the user typed after the word. The
    /// corrector backspaces over it and re-emits a copy.
    pub boundary_char: char,
    /// Scancode + shift of that boundary key, for faithful replay.
    /// Enter/Tab are substituted with Space at replay time — re-
    /// pressing those would submit a line / move focus.
    pub boundary_scancode: u32,
    pub boundary_shift: bool,
}

/// Result of one non-blocking sweep over the key channel during a
/// correction. See [`SwitcherEngine::drain_correction_window`].
#[derive(Default)]
pub struct WindowDrain {
    /// Plain word-key presses, in arrival order.
    pub word_keys: Vec<KeyEvent>,
    /// First boundary press encountered (drain stops there).
    pub resume: Option<KeyEvent>,
    /// Backspace / nav / click / shortcut seen — screen state unclear.
    pub suspicious: bool,
    /// Any non-echo user press seen at all (quiet-probe signal).
    pub saw_user_press: bool,
}
