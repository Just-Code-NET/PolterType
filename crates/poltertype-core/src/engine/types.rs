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
    /// One latch per digit key 1..=9 for the suggestion-accept chord.
    pub suggest_digit_down: [bool; 9],
}

/// Modifier half of the suggestion-accept chord, parsed once at offer
/// time from `[suggestions].accept_modifiers`. Matched exactly, like
/// [`Chord`]: extra held modifiers do not fire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcceptModifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
}

impl AcceptModifiers {
    /// Parse `"Ctrl+Shift"`-style strings. `None` for empty / junk
    /// input (keyboard accept disabled), and for the modifier-less
    /// case — bare digits must never trigger replacements.
    pub fn parse(s: &str) -> Option<Self> {
        let mut m = Self {
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        };
        for part in s.split('+').map(str::trim).filter(|p| !p.is_empty()) {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => m.ctrl = true,
                "shift" => m.shift = true,
                "alt" | "option" => m.alt = true,
                "meta" | "super" | "cmd" | "win" => m.meta = true,
                _ => return None,
            }
        }
        (m.ctrl || m.alt || m.meta).then_some(m)
    }
}

/// What accepting a suggestion entry does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionAction {
    /// Replace the mistyped word with the entry's text.
    Replace,
    /// Add the mistyped word to the user's dictionary overlay for
    /// this language — no text change; the word stops being flagged.
    /// The engine only emits the request; the app owns the overlay
    /// file and the dictionary reload.
    AddToDictionary,
}

/// One entry of a suggestion offer, as shown in the tooltip.
#[derive(Debug, Clone)]
pub struct SuggestionEntry {
    /// For [`SuggestionAction::Replace`]: the replacement text,
    /// capitalised to match the typed token. For
    /// [`SuggestionAction::AddToDictionary`]: the typed word itself
    /// (the UI shows its own label; the text rides along so the
    /// accept path knows what to add).
    pub text: String,
    /// `Some(layout)` when applying this entry also switches the
    /// keyboard layout — the below-confidence-threshold cross-layout
    /// candidate, offered here instead of auto-applied.
    pub switch_to: Option<LayoutId>,
    pub action: SuggestionAction,
}

/// A suggestion offer awaiting the user's accept (digit chord or
/// tooltip click). Mirrors [`LastWord`] — same screen-position
/// caveats — plus everything needed to validate a late accept.
/// Separators and any in-progress next word are NOT stored: the
/// accept path reads them from the live [`WordBuffer`] at accept
/// time, because they may legitimately change while the tooltip is
/// up (a second space, the next word's first letters).
///
/// [`WordBuffer`]: crate::engine::buffer::WordBuffer
#[derive(Debug, Clone)]
pub struct PendingSuggestion {
    /// Ties accepts/dismissals to this exact offer.
    pub generation: u64,
    pub keys: Vec<poltertype_types::WordKey>,
    pub rendered: String,
    pub layout: LayoutId,
    pub entries: Vec<SuggestionEntry>,
    /// Accepts after this instant are ignored (the tooltip is gone).
    pub deadline: std::time::Instant,
    /// Parsed accept chord; `None` = click-to-apply only.
    pub accept: Option<AcceptModifiers>,
    /// Screen state frozen the instant a pointer press was observed —
    /// see [`FrozenScreen`]. `None` until a click happens.
    pub frozen: Option<FrozenScreen>,
}

/// The buffer's screen model, captured *just before* a pointer press
/// abandons it.
///
/// Why this exists: a click lands in the key stream as
/// `SC_POINTER_BUTTON` and rightly abandons the buffer (the caret
/// usually moved). But a click *on the suggestion tooltip* never
/// reaches the app below — the overlay swallows it — so the text and
/// caret are exactly where they were. The tooltip's `Accepted` event
/// races the evdev observation of the same physical click, so the
/// engine freezes the deletion math here and honours an accept that
/// arrives within the short grace window; any other keypress, or the
/// window lapsing, voids it.
#[derive(Debug, Clone)]
pub struct FrozenScreen {
    /// Boundary keys after the offered word (`WordBuffer::boundary_run`).
    pub run: Vec<(u32, bool)>,
    /// In-progress next-word keys (`WordBuffer::keys`).
    pub tail: Vec<poltertype_types::WordKey>,
    /// Grace deadline — accepts after this are declined.
    pub until: std::time::Instant,
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
    /// The press that set `suspicious`, when it is one we could still
    /// re-emit (Backspace, arrows, Esc, Enter/Tab). A held correction
    /// swallowed it before the application saw it, so it has to be
    /// typed out rather than lost. `None` for shortcuts and pointer
    /// presses, which we have no faithful way to reproduce.
    pub stopper: Option<KeyEvent>,
    /// Any non-echo user press seen at all (quiet-probe signal).
    pub saw_user_press: bool,
}

/// RAII hold on the user's keyboard for the length of one emission
/// burst. Held keys are still delivered to the engine — they just do
/// not reach the focused application until this is dropped, so nothing
/// of the user's can land in the middle of the text we are typing.
///
/// Dropping releases, on every path out of a correction including a
/// panic. The backend enforces its own ceiling on top of that, so even
/// a leak here cannot leave the keyboard dead.
pub struct HeldKeys<'a> {
    gate: &'a poltertype_input::KeyGate,
    active: bool,
}

impl<'a> HeldKeys<'a> {
    /// Ask the gate to hold. `active()` reports whether it actually is
    /// — callers must stay correct when it isn't.
    pub fn acquire(gate: &'a poltertype_input::KeyGate) -> Self {
        Self {
            gate,
            active: gate.hold(),
        }
    }

    pub fn active(&self) -> bool {
        self.active
    }

    /// Let the user's keys through again, before the guard goes out of
    /// scope. Idempotent.
    pub fn release(&mut self) {
        if self.active {
            self.gate.release();
            self.active = false;
        }
    }
}

impl Drop for HeldKeys<'_> {
    fn drop(&mut self) {
        self.release();
    }
}
