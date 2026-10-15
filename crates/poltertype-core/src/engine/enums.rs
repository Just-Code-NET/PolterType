//! Engine message enums: outbound notifications, inbound commands, and
//! the small enums plain data types are built from.

use std::time::Duration;

use poltertype_layout::LayoutId;

use super::types::{Chord, KeystreamHotkeys, ModChord, SuggestionEntry};

/// Outbound notifications the engine emits.
#[derive(Debug, Clone)]
pub enum SwitcherEvent {
    /// Layout switched with no text change — the tray icon follows it.
    LayoutChanged(LayoutId),
    /// A correction went out: layout switched *and* the word retyped.
    Corrected {
        from_layout: LayoutId,
        to_layout: LayoutId,
        original_text: String,
        corrected_text: String,
        reason: String,
    },
    /// Paused or resumed — by hotkey or by the tray menu.
    PausedChanged(bool),
    /// Buffer examined, current layout kept — for debug overlays.
    KeptCurrent { reason: String },
    /// The app shows these in the suggestion tooltip. `generation` ties
    /// later accept / dismiss round-trips to exactly this offer; a
    /// stale generation is ignored.
    SuggestionsReady {
        generation: u64,
        original: String,
        entries: Vec<SuggestionEntry>,
        /// How long the tooltip should stay up (the popup owns the
        /// timer; the engine validates its own deadline on accept).
        timeout: Duration,
        /// Digit-chord hint for the tooltip footer, e.g.
        /// `"Ctrl+Shift"` — empty when keyboard accept is disabled.
        accept_modifiers: String,
        /// `F1`..`F9` accept instead of the digit chord.
        function_keys: bool,
        /// `Down` / `Up` move a highlight and `Enter` accepts it.
        arrows: bool,
    },
    /// The keyboard moved the tooltip's highlight to row `index` of
    /// offer `generation`. The offer's lifetime restarts with it, so
    /// the tooltip's timer should too.
    SuggestionHighlighted { generation: u64, index: usize },
    /// The offer identified by `generation` is no longer actionable
    /// (next word committed, caret moved, pause, …) — hide the
    /// tooltip if it is still showing it.
    SuggestionsDismissed { generation: u64 },
    /// A suggestion replaced `original` on screen.
    SuggestionApplied {
        original: String,
        replacement: String,
    },
    /// The engine owns no files: the app appends `word` to the user's
    /// wordlist overlay for `layout` and hot-swaps the dictionaries,
    /// after which the word stops being flagged and auto-corrected.
    AddToDictionary {
        layout: LayoutId,
        word: String,
        origin: DictionaryAddOrigin,
    },
    /// A tooltip offering "Add to dictionary" went away without being
    /// used. The app keeps a short list of these so the offer can be
    /// taken up later from the tray (issue #38): the tooltip is aimed
    /// at people who type fast, and the next keystroke dismisses it —
    /// so the faster you type, the less reachable it is.
    DictionaryOfferMissed { layout: LayoutId, word: String },
}

/// Why a word is being added to the user's dictionary. The app shows
/// the user a notification for the implicit route and stays quiet for
/// the explicit one — pressing a button labelled "Add to dictionary"
/// is its own confirmation, undoing a correction is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictionaryAddOrigin {
    Tooltip,
    /// The user took back a correction the engine had made, which
    /// says the word was right as typed.
    UndoneCorrection,
}

/// Commands sent into the engine from the app loop.
#[derive(Debug, Clone)]
pub enum EngineCommand {
    TogglePause,
    /// Put auto-switching in a named state instead of flipping it.
    /// `config.toml` carries the same state, and a toggle driven by a
    /// file lands the wrong way round whenever the two have drifted —
    /// which is exactly when the file is being applied.
    SetPaused(bool),
    /// Force a switch on the most recently completed word, ignoring
    /// the detector (Manual-switch-last hotkey).
    SwitchLastForcefully,
    /// Refresh whatever caches the engine keeps.
    SettingsReloaded,
    /// Enable (or update) hotkey detection straight off the key stream.
    /// Used on backends where the OS-level [`global-hotkey`] grab can't
    /// see input — notably Wayland, where the evdev listener is the only
    /// thing that observes `Ctrl+Shift+Space` at all. An empty value
    /// disables keystream detection (the OS grab is doing the job).
    SetKeystreamHotkeys(KeystreamHotkeys),
    /// Apply suggestion `index` of offer `generation`. Ignored when the
    /// generation is stale or the word is no longer replaceable.
    /// `from_pointer` marks tooltip clicks: exactly one physical click
    /// is then in flight in the key stream and must be tolerated by the
    /// correction's absorb machinery instead of aborting it as "caret
    /// moved".
    AcceptSuggestion {
        generation: u64,
        index: usize,
        from_pointer: bool,
    },
    /// The tooltip for offer `generation` went away on the popup side
    /// (timeout, Esc) — drop the engine's pending state to match.
    DismissSuggestions {
        generation: u64,
    },
}

pub enum Either<A, B> {
    Cmd(A),
    Key(B),
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

/// What a hotkey answers to on the key stream: an ordinary chord, or
/// modifiers alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binding {
    Key(Chord),
    Mods(ModChord),
}

/// What accepting a suggestion entry does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionAction {
    Replace,
    /// No text change. The engine only emits the request; the app owns
    /// the overlay file and the dictionary reload.
    AddToDictionary,
}
