//! The `SwitcherEngine` struct: state fields and construction.
//! Behaviour lives in the sibling files, one `impl` per concern.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::Instant;

use crossbeam_channel::Sender;
use parking_lot::{Mutex, RwLock};
use poltertype_detect::{Detector, SuggestionProvider};
use poltertype_input::{FocusTracker, KeyEmitter, KeyGate};
use poltertype_layout::{LayoutId, LayoutSwitcher};
use poltertype_types::Modifiers;

use crate::audio::AudioPlayer;
use crate::commands::WordHistory;
use crate::engine::enums::SwitcherEvent;
use crate::engine::types::{KeystreamHotkeys, LastWord, PendingSuggestion};
use crate::layouts::LayoutDb;
use crate::settings::SettingsStore;

pub struct SwitcherEngine {
    pub(super) settings: Arc<SettingsStore>,
    pub(super) layouts: Arc<LayoutDb>,
    pub(super) detectors: Vec<Box<dyn Detector>>,
    pub(super) layout_switcher: Arc<dyn LayoutSwitcher>,
    pub(super) key_emitter: Arc<dyn KeyEmitter>,
    /// Holds the user's keystrokes back while a correction burst is on
    /// the wire. A no-op gate (every platform but Linux/evdev, and
    /// stacks where grabbing would gag us instead) leaves the engine on
    /// its absorb-and-repair path.
    pub(super) key_gate: KeyGate,
    /// Modifiers the user was holding as of the last event we saw. A
    /// correction fired *by* a chord must let them go before replaying:
    /// under a held `Ctrl` every replayed key arrives as a shortcut and
    /// nothing is typed.
    pub(super) held_modifiers: RwLock<Modifiers>,
    pub(super) focus_tracker: Arc<dyn FocusTracker>,
    pub(super) audio: Arc<AudioPlayer>,
    pub(super) out_tx: Sender<SwitcherEvent>,
    pub(super) paused: Arc<RwLock<bool>>,
    /// The last few completed words, so a smart-command trigger can span
    /// more than one (`best regards`). Bounded by length, idle timeout
    /// and focus change, because this is the one place the engine holds
    /// more of the user's text than the word being typed. See
    /// [`crate::commands::phrase`].
    pub(super) word_history: Arc<RwLock<WordHistory>>,
    /// Buffer of the previous fully-completed word (for "switch-last").
    pub(super) last_word: Arc<RwLock<Option<LastWord>>>,
    /// Layout in effect when the in-progress word's first key arrived.
    ///
    /// The buffer holds scancodes, so what a word *reads* as depends on
    /// the layout active while it was typed — and the user may have
    /// switched by hand since. Without this stamp `decide` reads the
    /// word under whatever is active at the boundary, finds gibberish
    /// where the screen holds good text, and "corrects" it: retyping the
    /// word and dragging the layout off the one the user just chose.
    ///
    /// `None` when the OS could not be asked, read as "assume it never
    /// changed".
    pub(super) word_layout: RwLock<Option<LayoutId>>,
    /// Expected echoes of our own injected keystrokes: the scancode of
    /// every *press* the emitter put on the wire, oldest first, each
    /// with an expiry deadline.
    ///
    /// keyd and friends proxy our events through their own virtual
    /// keyboard, stripping the `injected` marker; left unguarded the
    /// engine reads its own replay back, corrects it again, and spirals
    /// into a backspace+space loop.
    ///
    /// Match-and-consume rather than a blanket suppression window:
    /// suppressing everything for 300–400 ms ate the first real
    /// keystrokes of the next word for fast typists, so the next
    /// correction under-counted its backspaces and left the leading
    /// characters behind.
    ///
    /// Releases are exempt: they are state-neutral downstream, and
    /// remappers sometimes filter ours, which would desync the queue.
    pub(super) expected_echo: Mutex<VecDeque<(u32, Instant)>>,
    /// Hotkey chords matched directly off the key stream. Empty unless
    /// the app enables them (Wayland) via
    /// [`EngineCommand::SetKeystreamHotkeys`](crate::engine::enums::EngineCommand::SetKeystreamHotkeys).
    pub(super) keystream_hotkeys: RwLock<KeystreamHotkeys>,
    /// Spelling-suggestion provider (`None` = feature not wired /
    /// disabled at construction). See `docs/PLAN.md` §3.8.B — this is
    /// the suggestion seam the AI subsystem can later replace.
    pub(super) suggester: Option<Arc<dyn SuggestionProvider>>,
    /// The one in-flight suggestion offer, if any. Generation-stamped
    /// so a stale tooltip click can never replace the wrong word.
    pub(super) pending_suggestion: Mutex<Option<PendingSuggestion>>,
    /// Monotonic stamp source for [`PendingSuggestion::generation`].
    pub(super) suggestion_generation: AtomicU64,
    /// Deadline before which auto-correction is suppressed because the
    /// user just pasted (Ctrl+V / Ctrl+Shift+V / Shift+Insert).
    ///
    /// A paste is not typing and must never be retyped into another
    /// layout, but on Wayland a compositor or remapper can replay the
    /// inserted text through a virtual keyboard, indistinguishable from
    /// human typing event by event. Hence a window rather than a filter.
    /// The buffer keeps tracking, so correction resumes the moment it
    /// lapses.
    pub(super) paste_guard_until: RwLock<Instant>,
}

/// Everything the engine is built out of.
///
/// A struct rather than positional parameters because seven of these are
/// `Arc<dyn …>` trait objects: any two of the same shape transpose at
/// the call site and still compile. Named fields make the wiring in
/// `main.rs` impossible to get wrong that way.
pub struct EngineDeps {
    pub settings: Arc<SettingsStore>,
    pub layouts: Arc<LayoutDb>,
    pub detectors: Vec<Box<dyn Detector>>,
    pub layout_switcher: Arc<dyn LayoutSwitcher>,
    pub key_emitter: Arc<dyn KeyEmitter>,
    pub key_gate: KeyGate,
    pub focus_tracker: Arc<dyn FocusTracker>,
    pub audio: Arc<AudioPlayer>,
    pub out_tx: Sender<SwitcherEvent>,
    /// `None` when no suggestion provider is wired — the feature is
    /// then inert, not merely disabled.
    pub suggester: Option<Arc<dyn SuggestionProvider>>,
}

impl SwitcherEngine {
    pub fn new(deps: EngineDeps) -> Self {
        let EngineDeps {
            settings,
            layouts,
            detectors,
            layout_switcher,
            key_emitter,
            key_gate,
            focus_tracker,
            audio,
            out_tx,
            suggester,
        } = deps;
        Self {
            settings,
            layouts,
            detectors,
            layout_switcher,
            key_emitter,
            key_gate,
            held_modifiers: RwLock::new(Modifiers::NONE),
            focus_tracker,
            audio,
            out_tx,
            paused: Arc::new(RwLock::new(false)),
            word_history: Arc::new(RwLock::new(WordHistory::default())),
            last_word: Arc::new(RwLock::new(None)),
            word_layout: RwLock::new(None),
            expected_echo: Mutex::new(VecDeque::new()),
            keystream_hotkeys: RwLock::new(KeystreamHotkeys::default()),
            suggester,
            pending_suggestion: Mutex::new(None),
            suggestion_generation: AtomicU64::new(0),
            paste_guard_until: RwLock::new(Instant::now()),
        }
    }

    pub fn paused(&self) -> bool {
        *self.paused.read()
    }
}
