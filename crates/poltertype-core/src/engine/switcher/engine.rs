//! The `SwitcherEngine` struct: state fields and construction.
//! Behaviour lives in the sibling files, one `impl` per concern.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::Instant;

use crossbeam_channel::Sender;
use parking_lot::{Mutex, RwLock};
use poltertype_detect::{Detector, SuggestionProvider};
use poltertype_input::{FocusTracker, KeyEmitter};
use poltertype_layout::LayoutSwitcher;

use crate::audio::AudioPlayer;
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
    pub(super) focus_tracker: Arc<dyn FocusTracker>,
    pub(super) audio: Arc<AudioPlayer>,
    pub(super) out_tx: Sender<SwitcherEvent>,
    pub(super) paused: Arc<RwLock<bool>>,
    /// Buffer of the previous fully-completed word (for "switch-last").
    pub(super) last_word: Arc<RwLock<Option<LastWord>>>,
    /// Expected echoes of our own injected keystrokes: scancodes of
    /// every *press* the emitter reported putting on the wire, oldest
    /// first, each with an expiry deadline.
    ///
    /// On Linux/Wayland the only correction path that actually works
    /// inside terminals and Wayland-native apps is to replay the
    /// original scancodes via uinput *after* `switch_to`. But our
    /// uinput device is not distinguishable from a real keyboard at
    /// the listener level — keyd (and similar input remappers) proxies
    /// our virtual events through its own virtual keyboard, stripping
    /// the `injected` marker entirely. Without protection the engine
    /// would read its own replay back, run another correction on it,
    /// and spiral into an infinite backspace+space loop.
    ///
    /// Earlier versions suppressed *everything* for a fixed 300-400 ms
    /// window after a correction and cleared the word buffer on every
    /// event inside it. That ate the first real keystrokes of the next
    /// word for fast typists: the characters were on screen but not in
    /// the buffer, so the *next* correction under-counted its
    /// backspaces and left the leading characters behind — the
    /// "перший символ слова залишається" bug. Match-and-consume is
    /// precise instead: each incoming press either matches the head of
    /// this queue (→ it's our echo, swallow it) or is real user input
    /// (→ process normally, no matter how soon after a correction).
    /// Only releases are exempt — they are state-neutral everywhere
    /// downstream and remappers sometimes filter ours, so tracking
    /// them would desync the queue.
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
    /// Wall-clock deadline before which auto-correction is suppressed
    /// because the user just pasted (Ctrl+V / Ctrl+Shift+V / Shift+Insert).
    ///
    /// A clipboard paste is not "typing", so its text must never be
    /// retyped into another layout. On most backends the pasted content
    /// never reaches us as key events at all. But on Wayland the
    /// compositor / input remapper (keyd & friends) can replay the
    /// inserted text through a virtual keyboard, where it is
    /// indistinguishable from human typing — the engine would then
    /// "correct" a word the user never typed. We can't tell those
    /// synthetic keystrokes apart event-by-event, so instead we mark a
    /// short window after the paste shortcut and decline to auto-correct
    /// anything that completes inside it. The buffer still tracks keys,
    /// so normal correction resumes the moment the window lapses.
    pub(super) paste_guard_until: RwLock<Instant>,
}

impl SwitcherEngine {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        settings: Arc<SettingsStore>,
        layouts: Arc<LayoutDb>,
        detectors: Vec<Box<dyn Detector>>,
        layout_switcher: Arc<dyn LayoutSwitcher>,
        key_emitter: Arc<dyn KeyEmitter>,
        focus_tracker: Arc<dyn FocusTracker>,
        audio: Arc<AudioPlayer>,
        out_tx: Sender<SwitcherEvent>,
        suggester: Option<Arc<dyn SuggestionProvider>>,
    ) -> Self {
        Self {
            settings,
            layouts,
            detectors,
            layout_switcher,
            key_emitter,
            focus_tracker,
            audio,
            out_tx,
            paused: Arc::new(RwLock::new(false)),
            last_word: Arc::new(RwLock::new(None)),
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
