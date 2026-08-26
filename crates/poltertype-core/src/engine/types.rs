//! Plain data carried around by the engine: hotkey chords, the
//! stashed last word, and the correction-window drain summary.

use std::time::Instant;

use poltertype_input::{KeyEvent, ReplayKey};
use poltertype_layout::LayoutId;
use poltertype_types::WordKey;

/// One correction, described in full: what to delete, what to type in
/// its place, and under which layout.
///
/// Named fields because four of these are `&str`/`&LayoutId` pairs that
/// transpose silently — a swapped `from`/`to` compiles, type-checks, and
/// then deletes the wrong number of characters under the wrong layout.
pub struct Correction<'a> {
    /// Layout the text was typed under. Equal to [`Self::to`] for a
    /// same-layout replacement (a spelling suggestion), and that
    /// equality is what the emitter keys "is this a switch?" off.
    pub from: &'a LayoutId,
    pub to: &'a LayoutId,
    /// What is on screen now — for the event payload and the log.
    pub original: &'a str,
    pub corrected: &'a str,
    /// How many characters to delete before typing the replacement.
    pub backspaces: usize,
    /// Why this is happening, for the log and the emitted event.
    pub reason: &'a str,
    pub play_sound: bool,
    /// Scancodes to replay, when the replacement can be typed as keys
    /// rather than injected as text.
    pub replay_keys: Option<&'a [ReplayKey]>,
    /// How many pointer presses the absorb machinery may swallow
    /// instead of reading as "the caret moved". Zero everywhere except
    /// a tooltip-click accept, where exactly one physical click — the
    /// one that hit the tooltip, an overlay the app below never saw —
    /// is still in flight in the key stream.
    pub pointer_click_allowance: usize,
}

/// An accepted suggestion, worked out down to what the emitter needs.
/// Separate from applying it so every reason to decline lands before
/// anything reaches the screen.
pub struct PlannedReplacement {
    pub target_layout: LayoutId,
    /// On-screen characters to delete: the word, the separator run,
    /// the in-progress next word, and the chord digit if one was typed.
    pub backspaces: usize,
    /// The full replacement as rendered text — event payload, and the
    /// body of the text-injection fallback.
    pub corrected: String,
    /// Scancodes for the replacement, when it can be typed rather than
    /// injected.
    pub replay: Option<Vec<ReplayKey>>,
    pub reason: &'static str,
    /// The replacement word in scancode form under the target layout,
    /// used to re-point the word buffer's stash so backspacing across
    /// the boundary re-opens the right thing. `None` when the layout
    /// cannot type every character and text injection was used.
    pub replacement_keys: Option<Vec<WordKey>>,
}

/// A resolved hotkey chord matched against the raw key stream.
///
/// `scancode` is Win SC Set-1. Modifier fields match exactly: extra
/// held modifiers do *not* match, so `Ctrl+Shift+Space` never fires on
/// `Ctrl+Shift+Alt+Space`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
    pub scancode: u32,
}

/// Which modifier a bare modifier key stands for. Left and right keys
/// carry the same role: nobody binds "the left Shift".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModRole {
    Ctrl,
    Shift,
    Alt,
    Meta,
}

/// A set of modifier roles — the whole of a modifier-only chord.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModSet {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
}

impl ModSet {
    pub const NONE: Self = Self {
        ctrl: false,
        shift: false,
        alt: false,
        meta: false,
    };

    pub fn is_empty(self) -> bool {
        self == Self::NONE
    }

    pub fn count(self) -> usize {
        usize::from(self.ctrl)
            + usize::from(self.shift)
            + usize::from(self.alt)
            + usize::from(self.meta)
    }

    #[must_use]
    pub fn with(mut self, role: ModRole) -> Self {
        *self.slot(role) = true;
        self
    }

    #[must_use]
    pub fn without(mut self, role: ModRole) -> Self {
        *self.slot(role) = false;
        self
    }

    pub fn contains(self, role: ModRole) -> bool {
        match role {
            ModRole::Ctrl => self.ctrl,
            ModRole::Shift => self.shift,
            ModRole::Alt => self.alt,
            ModRole::Meta => self.meta,
        }
    }

    fn slot(&mut self, role: ModRole) -> &mut bool {
        match role {
            ModRole::Ctrl => &mut self.ctrl,
            ModRole::Shift => &mut self.shift,
            ModRole::Alt => &mut self.alt,
            ModRole::Meta => &mut self.meta,
        }
    }
}

/// A hotkey made of modifier keys alone — the Punto / Caramba gesture
/// the hands already know (issue #32).
///
/// It cannot be an OS-level grab: `HotKey` needs a key code, and there
/// is no key here. It is matched off the raw key stream on every
/// backend instead, under the rule that keeps `Ctrl+C` from firing it:
/// the chord acts on *release*, and only if nothing else was pressed
/// while the modifiers were down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModChord {
    pub mods: ModSet,
    /// Two taps rather than one — `Shift+Shift`. A single lone Shift is
    /// deliberately not offered: Shift+click is invisible to us on
    /// Windows and macOS, so a one-tap binding would fire on a
    /// selection.
    pub double_tap: bool,
}

/// What a hotkey answers to on the key stream: an ordinary chord, or
/// modifiers alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binding {
    Key(Chord),
    Mods(ModChord),
}

/// The two engine hotkeys, resolved to key-stream bindings. `None`
/// means "not matched here" — either unbound, or held by the OS-level
/// grab, which never coexists with a key-stream match for the same
/// hotkey.
#[derive(Debug, Clone, Copy, Default)]
pub struct KeystreamHotkeys {
    pub pause: Option<Binding>,
    pub switch_last: Option<Binding>,
}

/// Per-chord rising-edge tracking. evdev reports autorepeat as repeated
/// presses, so we latch on the first press and only re-arm on release —
/// one fire per physical keypress, no matter how long it's held.
#[derive(Default)]
pub struct ChordState {
    pub pause: BindingState,
    pub switch: BindingState,
    /// One latch per digit key 1..=9 for the suggestion-accept chord.
    pub suggest_digit_down: [bool; 9],
}

/// Everything one hotkey has to remember between key events.
#[derive(Default)]
pub struct BindingState {
    pub key_down: bool,
    pub mods: ModTapState,
}

/// The modifier-only chord's view of one hold: what came down, whether
/// anything else did, and when it started.
#[derive(Default)]
pub struct ModTapState {
    /// Modifier keys physically down right now.
    pub down: ModSet,
    /// Every modifier seen during this hold — a chord is judged on what
    /// was held together, not on what is left at the last release.
    pub peak: ModSet,
    /// A non-modifier key (or a mouse button, where we see them) was
    /// pressed during the hold, so this is a shortcut, not a tap.
    pub dirty: bool,
    /// When the hold began. `None` while no modifier is down.
    pub started: Option<Instant>,
    /// When the previous qualifying tap ended, for the double-tap gap.
    pub last_tap: Option<Instant>,
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
    Replace,
    /// No text change. The engine only emits the request; the app owns
    /// the overlay file and the dictionary reload.
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

/// A suggestion offer awaiting the user's accept. Separators and any
/// in-progress next word are deliberately absent: they may legitimately
/// change while the tooltip is up, so they are read from the live
/// [`WordBuffer`] at accept time.
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
/// A click rightly abandons the buffer, since the caret usually moved —
/// but a click *on the tooltip* never reaches the app below, so text and
/// caret are exactly where they were. The tooltip's `Accepted` event
/// races the evdev observation of that same click, so the deletion math
/// is frozen here and honoured within a short grace window.
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
    /// The key that closed the word, or `None` while it is still being
    /// typed — which is when most people reach for the manual hotkey,
    /// having arrived from Punto Switcher or Caramba where the gesture
    /// acts on the word under the fingers.
    pub boundary: Option<WordBoundaryKey>,
    /// The layout the engine's own correction moved this word to, once
    /// it has landed on screen; `None` while the word still reads as
    /// typed. This is how the manual switch-last hotkey tells its two
    /// situations apart: `None` means apply the switch the engine
    /// declined, `Some` means undo the one it made.
    pub corrected_to: Option<LayoutId>,
}

/// The separator that closed a word, kept so a correction can put an
/// identical one back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WordBoundaryKey {
    /// The character itself. The corrector backspaces over it and
    /// re-emits a copy.
    pub ch: char,
    /// Scancode + shift, for faithful replay. Enter/Tab are substituted
    /// with Space at replay time — re-pressing those would submit a
    /// line or move focus.
    pub scancode: u32,
    pub shift: bool,
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
    /// re-emit (Backspace, arrows, Esc, Enter/Tab): a held correction
    /// swallowed it before the application saw it. `None` for shortcuts
    /// and pointer presses, which have no faithful reproduction.
    pub stopper: Option<KeyEvent>,
    /// Any non-echo user press seen at all (quiet-probe signal).
    pub saw_user_press: bool,
}

/// RAII hold on the user's keyboard for one emission burst. Held keys
/// still reach the engine — they just do not reach the focused
/// application until this is dropped. The backend enforces its own
/// ceiling on top, so even a leak here cannot leave the keyboard dead.
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
