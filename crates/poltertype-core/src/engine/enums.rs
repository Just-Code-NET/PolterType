//! Engine message enums: outbound notifications and inbound commands.

use std::time::Duration;

use poltertype_layout::LayoutId;

use super::types::{KeystreamHotkeys, SuggestionEntry};

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
    },
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
        /// The accept came from a digit chord matched off the key
        /// stream, so the digit itself was typed into the document on
        /// the way past and now sits left of the caret — it has to be
        /// erased along with the word. Chords are matched, not
        /// grabbed: registering nine global hotkeys would steal those
        /// combinations from every application.
        typed_digit: bool,
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
