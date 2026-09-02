//! Engine unit + integration tests.
//!
//! This prelude re-imports the engine's public API plus the internal
//! submodules, so the inner test modules resolve names through
//! `use super::*`.

use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use parking_lot::Mutex;
use poltertype_detect::{Detector, Verdict};
use poltertype_input::{EmittedKey, InputError, KeyDirection, KeyEmitter, KeyEvent};
use poltertype_layout::LayoutId;

use super::consts::*;
use super::heuristics::*;
use super::types::*;
use super::*;

/// Full-engine integration tests with mocked OS surfaces. They drive
/// `SwitcherEngine::run` on a real thread through the public channel
/// API, the way `poltertype-app` does, and assert on the exact key
/// operations emitted — the regression net for keystrokes racing a
/// correction and for the word head lost across a backspace-over-
/// boundary edit.
mod engine_integration_tests {
    use super::*;
    use crate::layouts::LayoutDb;
    use crate::settings::SettingsStore;
    use poltertype_input::{NoopFocusTracker, ReplayKey};
    use poltertype_layout::LayoutError;
    use poltertype_types::DetectionVerdict;
    use std::sync::Arc;
    use std::thread::JoinHandle;

    // ─── Mocks ───────────────────────────────────────────────────────

    #[derive(Debug, Clone, PartialEq)]
    enum EmitOp {
        Backspaces(usize),
        Keys(Vec<u32>), // scancodes only, shift not asserted here
        Text(String),
        ReleaseModifiers,
    }

    /// Fires from inside a replay burst — see `MockEmitter::during_replay`.
    type ReplayHook = Box<dyn Fn() + Send>;

    /// Fires as a chord goes out — see `MockEmitter::on_chord`. The
    /// stand-in for the focused application, which is what actually
    /// answers a copy chord by filling the clipboard.
    type ChordHook = Box<dyn Fn(poltertype_types::SwitchChord) + Send>;

    /// A clipboard that behaves like a desktop's: what is written stays
    /// readable until something else is written.
    ///
    /// The engine never puts a *selection* here itself — it presses
    /// copy and waits to see whether anything arrives, so the
    /// application's half of that is `MockEmitter::on_chord`.
    #[derive(Default)]
    struct MockClipboard {
        text: Mutex<Option<String>>,
        /// Every value written, in order. The restore is as much of
        /// this feature's contract as the staging is: the user's
        /// clipboard has to come back.
        writes: Mutex<Vec<String>>,
    }

    impl MockClipboard {
        fn writes(&self) -> Vec<String> {
            self.writes.lock().clone()
        }
    }

    impl poltertype_input::Clipboard for MockClipboard {
        fn text(&self) -> Result<Option<String>, InputError> {
            Ok(self.text.lock().clone())
        }

        fn set_text(&self, text: &str) -> Result<(), InputError> {
            *self.text.lock() = Some(text.to_owned());
            self.writes.lock().push(text.to_owned());
            Ok(())
        }
    }

    /// Records every operation and mimics the uinput emitter's echo log
    /// (press+release per backspace / replay key, shift presses
    /// included) so tests can replay realistic keyd-style echoes.
    /// `emitted` is drained by the engine's `take_emitted`; `echo_copy`
    /// is the test's own copy to replay from.
    #[derive(Default)]
    struct MockEmitter {
        ops: Mutex<Vec<EmitOp>>,
        emitted: Mutex<Vec<EmittedKey>>,
        echo_copy: Mutex<Vec<EmittedKey>>,
        /// Every replay burst with its shift levels intact —
        /// `EmitOp::Keys` keeps scancodes only, and the boundary key's
        /// shift level is the whole point of
        /// `boundary_character_survives_the_layout_flip`.
        replays: Mutex<Vec<Vec<(u32, bool)>>>,
        /// Called from `send_keys` once the burst is on the wire: a
        /// test's stand-in for a physical keystroke the compositor
        /// interleaves with our replay.
        during_replay: Mutex<Option<ReplayHook>>,
        /// Every desktop switch chord this emitter was asked to send.
        chords: Mutex<Vec<poltertype_types::SwitchChord>>,
        /// Called as a chord goes out — the focused application's half
        /// of a copy. See `ChordHook`.
        on_chord: Mutex<Option<ChordHook>>,
    }

    impl MockEmitter {
        fn log(&self, sc: u32, dir: KeyDirection) {
            let e = EmittedKey {
                scancode: sc,
                direction: dir,
            };
            self.emitted.lock().push(e);
            self.echo_copy.lock().push(e);
        }
        fn ops(&self) -> Vec<EmitOp> {
            self.ops.lock().clone()
        }
    }

    impl KeyEmitter for MockEmitter {
        fn send_chord(&self, chord: poltertype_types::SwitchChord) -> Result<(), InputError> {
            self.chords.lock().push(chord);
            if let Some(hook) = self.on_chord.lock().as_ref() {
                hook(chord);
            }
            Ok(())
        }

        fn send_backspaces(&self, n: usize) -> Result<(), InputError> {
            self.ops.lock().push(EmitOp::Backspaces(n));
            for _ in 0..n {
                self.log(0x0E, KeyDirection::Press);
                self.log(0x0E, KeyDirection::Release);
            }
            Ok(())
        }

        fn send_text(&self, text: &str) -> Result<(), InputError> {
            self.ops.lock().push(EmitOp::Text(text.to_owned()));
            Ok(())
        }

        fn send_keys(&self, keys: &[ReplayKey]) -> Result<(), InputError> {
            self.ops
                .lock()
                .push(EmitOp::Keys(keys.iter().map(|k| k.scancode).collect()));
            self.replays
                .lock()
                .push(keys.iter().map(|k| (k.scancode, k.shift)).collect());
            for k in keys {
                if k.shift {
                    self.log(0x2A, KeyDirection::Press);
                }
                self.log(k.scancode, KeyDirection::Press);
                self.log(k.scancode, KeyDirection::Release);
                if k.shift {
                    self.log(0x2A, KeyDirection::Release);
                }
            }
            if let Some(hook) = self.during_replay.lock().as_ref() {
                hook();
            }
            Ok(())
        }

        fn release_modifiers(&self, _held: poltertype_types::Modifiers) -> Result<(), InputError> {
            self.ops.lock().push(EmitOp::ReleaseModifiers);
            Ok(())
        }

        fn take_emitted(&self) -> Vec<EmittedKey> {
            std::mem::take(&mut *self.emitted.lock())
        }

        fn backend_name(&self) -> &'static str {
            "mock"
        }
    }

    struct MockSwitcher {
        current: Mutex<LayoutId>,
        active: Vec<LayoutId>,
        switches: Mutex<Vec<LayoutId>>,
        fail_switch: bool,
        /// Simulates a desktop whose settings daemon puts the layout
        /// back before a key can go out — MATE, measured 2026-08-24.
        /// The switch is recorded, `current` is not moved.
        revert: Mutex<bool>,
        /// The chord this desktop answers to, when it has one.
        chord: Mutex<Option<poltertype_types::SwitchChord>>,
    }

    impl MockSwitcher {
        fn new(current: &str, active: &[&str]) -> Self {
            Self {
                current: Mutex::new(LayoutId::from(current)),
                active: active.iter().map(|s| LayoutId::from(*s)).collect(),
                switches: Mutex::new(Vec::new()),
                revert: Mutex::new(false),
                chord: Mutex::new(None),
                fail_switch: false,
            }
        }
    }

    impl poltertype_layout::LayoutSwitcher for MockSwitcher {
        fn current(&self) -> Result<LayoutId, LayoutError> {
            Ok(self.current.lock().clone())
        }
        fn list_active(&self) -> Result<Vec<LayoutId>, LayoutError> {
            Ok(self.active.clone())
        }
        fn switch_to(&self, id: &LayoutId) -> Result<(), LayoutError> {
            if self.fail_switch {
                return Err(LayoutError::Os("test-forced failure".into()));
            }
            self.switches.lock().push(id.clone());
            if !*self.revert.lock() {
                *self.current.lock() = id.clone();
            }
            Ok(())
        }
        fn verify_switched(&self, target: &LayoutId) -> Option<bool> {
            Some(*self.current.lock() == *target)
        }
        fn switch_chord(&self) -> Option<poltertype_types::SwitchChord> {
            *self.chord.lock()
        }
        fn backend_name(&self) -> &'static str {
            "mock"
        }
    }

    /// Always votes to switch to "the other" of the two given layouts
    /// with full confidence — keeps decisions deterministic without
    /// dragging dictionaries into the tests.
    struct AlwaysOther(LayoutId, LayoutId);

    impl Detector for AlwaysOther {
        fn name(&self) -> &'static str {
            "test-always-other"
        }
        fn judge(&self, ctx: &poltertype_detect::DetectionContext<'_>) -> Verdict {
            let target = if *ctx.current_layout == self.0 {
                self.1.clone()
            } else {
                self.0.clone()
            };
            Verdict::Switch(DetectionVerdict {
                best_layout: target,
                confidence: 1.0,
                reason: "test".into(),
            })
        }
    }

    // ─── Harness ─────────────────────────────────────────────────────

    struct Harness {
        key_tx: Sender<KeyEvent>,
        cmd_tx: Sender<EngineCommand>,
        out_rx: Receiver<SwitcherEvent>,
        emitter: Arc<MockEmitter>,
        switcher: Arc<MockSwitcher>,
        engine_thread: JoinHandle<()>,
        /// What the engine would have played. Nothing drains it, so a
        /// test reads the whole run at once.
        audio_rx: Receiver<crate::audio::AudioCmd>,
    }

    impl Harness {
        fn start(idle_timeout_ms: u64) -> Self {
            Self::start_with(idle_timeout_ms, MockEmitter::default(), false)
        }

        fn start_with(idle_timeout_ms: u64, emitter: MockEmitter, fail_switch: bool) -> Self {
            Self::start_full(idle_timeout_ms, emitter, fail_switch, None, None)
        }

        fn start_full(
            idle_timeout_ms: u64,
            emitter: MockEmitter,
            fail_switch: bool,
            suggester: Option<Arc<dyn poltertype_detect::SuggestionProvider>>,
            detectors_override: Option<Vec<Box<dyn Detector>>>,
        ) -> Self {
            Self::start_tuned(
                idle_timeout_ms,
                emitter,
                fail_switch,
                suggester,
                detectors_override,
                None,
            )
        }

        /// `accept_modifiers` overrides the suggestion-accept chord, so
        /// a test can run the exact combination a user configured.
        fn start_tuned(
            idle_timeout_ms: u64,
            emitter: MockEmitter,
            fail_switch: bool,
            suggester: Option<Arc<dyn poltertype_detect::SuggestionProvider>>,
            detectors_override: Option<Vec<Box<dyn Detector>>>,
            accept_modifiers: Option<&str>,
        ) -> Self {
            Self::start_configured(
                idle_timeout_ms,
                emitter,
                fail_switch,
                suggester,
                detectors_override,
                accept_modifiers,
                |_| {},
                None,
            )
        }

        /// The widest constructor: `tweak` gets the whole `Settings`
        /// before the engine starts, for anything no narrower parameter
        /// covers, and `clipboard` installs one for the selection path.
        ///
        /// One argument over the lint's limit, and the narrower
        /// constructors above are exactly what keeps that from
        /// reaching the tests.
        #[allow(clippy::too_many_arguments)]
        fn start_configured(
            idle_timeout_ms: u64,
            emitter: MockEmitter,
            fail_switch: bool,
            suggester: Option<Arc<dyn poltertype_detect::SuggestionProvider>>,
            detectors_override: Option<Vec<Box<dyn Detector>>>,
            accept_modifiers: Option<&str>,
            tweak: impl FnOnce(&mut crate::settings::Settings),
            clipboard: Option<Arc<dyn poltertype_input::Clipboard>>,
        ) -> Self {
            let mut settings = crate::settings::Settings::default();
            settings.engine.idle_timeout_ms = idle_timeout_ms;
            if let Some(m) = accept_modifiers {
                settings.suggestions.accept_modifiers = m.to_owned();
            }
            tweak(&mut settings);
            let settings = Arc::new(SettingsStore::for_tests(settings));
            // The same two layouts the mock OS reports as active, and
            // only those: the real app loads exactly the active list,
            // and a bundled layout it would never load still decides
            // which keys count as letters. bg-BG puts `б` on the `/`
            // key, which quietly made `/tmp` one four-key token here
            // while a real en-US + uk-UA machine sees a path.
            let active = [LayoutId::from("en-US"), LayoutId::from("uk-UA")];
            let layouts = Arc::new(
                LayoutDb::load(crate::layouts::LoadOptions {
                    active_filter: Some(&active),
                    ..Default::default()
                })
                .expect("bundled layouts load"),
            );
            let emitter = Arc::new(emitter);
            let mut switcher = MockSwitcher::new("en-US", &["en-US", "uk-UA"]);
            switcher.fail_switch = fail_switch;
            let switcher = Arc::new(switcher);
            let detectors: Vec<Box<dyn Detector>> = detectors_override.unwrap_or_else(|| {
                vec![Box::new(AlwaysOther(
                    LayoutId::from("en-US"),
                    LayoutId::from("uk-UA"),
                ))]
            });
            let (audio, audio_rx) = crate::audio::AudioPlayer::for_tests();
            let (key_tx, key_rx) = crossbeam_channel::bounded::<KeyEvent>(1024);
            let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded::<EngineCommand>();
            let (out_tx, out_rx) = crossbeam_channel::unbounded::<SwitcherEvent>();
            let engine = SwitcherEngine::new(EngineDeps {
                settings: Arc::clone(&settings),
                layouts,
                detectors,
                layout_switcher: Arc::<MockSwitcher>::clone(&switcher)
                    as Arc<dyn poltertype_layout::LayoutSwitcher>,
                key_emitter: Arc::<MockEmitter>::clone(&emitter) as Arc<dyn KeyEmitter>,
                // `None` unless a test asked for one: selection
                // conversion is off by default, which is also its
                // shipped default.
                clipboard,
                // The gate is a no-op in tests: these exercise the
                // path taken when keystrokes cannot be held back.
                key_gate: poltertype_input::KeyGate::disabled(),
                focus_tracker: Arc::new(NoopFocusTracker),
                audio: Arc::new(audio),
                out_tx,
                suggester,
            });
            let engine_thread = std::thread::spawn(move || engine.run(key_rx, cmd_rx));
            Self {
                key_tx,
                cmd_tx,
                out_rx,
                emitter,
                switcher,
                engine_thread,
                audio_rx,
            }
        }

        /// Every sound the engine asked for, in order.
        fn sounds(&self) -> Vec<crate::audio::SoundEvent> {
            self.audio_rx
                .try_iter()
                .filter_map(|c| match c {
                    crate::audio::AudioCmd::Play(e) => Some(e),
                    _ => None,
                })
                .collect()
        }

        fn press(&self, sc: u32) {
            self.key(sc, KeyDirection::Press, false);
        }

        fn release(&self, sc: u32) {
            self.key(sc, KeyDirection::Release, false);
        }

        fn tap(&self, sc: u32) {
            self.press(sc);
            self.release(sc);
        }

        fn key(&self, sc: u32, direction: KeyDirection, shift: bool) {
            self.key_mods(
                sc,
                direction,
                poltertype_types::Modifiers {
                    shift,
                    ..poltertype_types::Modifiers::NONE
                },
            );
        }

        fn key_mods(
            &self,
            sc: u32,
            direction: KeyDirection,
            modifiers: poltertype_types::Modifiers,
        ) {
            self.key_tx
                .send(KeyEvent {
                    vk: sc,
                    scancode: sc,
                    direction,
                    modifiers,
                    injected: false,
                    timestamp_ms: 0,
                })
                .expect("engine alive");
        }

        /// Block until an event matching `pred` arrives (draining and
        /// discarding everything before it), or panic after ~5 s.
        fn wait_for(&self, pred: impl Fn(&SwitcherEvent) -> bool) -> SwitcherEvent {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let left = deadline.saturating_duration_since(Instant::now());
                match self.out_rx.recv_timeout(left) {
                    Ok(ev) if pred(&ev) => return ev,
                    Ok(_) => continue,
                    Err(_) => panic!("expected event never arrived"),
                }
            }
        }

        /// Wait until the engine has drained everything sent AND its
        /// emit-op log has stopped moving. Corrections deliberately
        /// dawdle (quiet-gap absorption, echo settle, chained
        /// decisions), so the stability window must outlast the
        /// engine's longest internal quiet stretch.
        fn settle(&self) {
            let mut last_ops = usize::MAX;
            let mut stable = 0;
            for _ in 0..600 {
                let ops_now = self.emitter.ops.lock().len();
                if self.key_tx.is_empty() && ops_now == last_ops {
                    stable += 1;
                    if stable >= 14 {
                        return;
                    }
                } else {
                    stable = 0;
                }
                last_ops = ops_now;
                std::thread::sleep(Duration::from_millis(100));
            }
            panic!("engine never settled");
        }

        /// Wait until the emitter has recorded at least `n` operations.
        /// Times echo replays realistically: echoes arrive while the
        /// engine is still inside its post-replay settle window, not
        /// seconds later.
        fn wait_ops(&self, n: usize) {
            for _ in 0..400 {
                if self.emitter.ops.lock().len() >= n {
                    return;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            panic!("emitter never reached {n} ops");
        }

        /// Feed the emitter's logged events back as keyd-style echoes:
        /// same scancodes, `injected = false`, presses and releases.
        fn replay_echoes(&self) {
            let echoes = std::mem::take(&mut *self.emitter.echo_copy.lock());
            for e in echoes {
                self.key(e.scancode, e.direction, false);
            }
        }

        fn stop(self) -> (Vec<EmitOp>, Vec<SwitcherEvent>) {
            drop(self.key_tx);
            drop(self.cmd_tx);
            self.engine_thread.join().expect("engine thread");
            let ops = self.emitter.ops();
            let events = self.out_rx.try_iter().collect();
            (ops, events)
        }
    }

    /// Scancodes for "ghbdsn" (how `привіт` comes out under en-US).
    const GHBDSN: [u32; 6] = [0x22, 0x23, 0x30, 0x20, 0x1F, 0x31];
    const SPACE: u32 = 0x39;
    const BACKSPACE: u32 = 0x0E;

    /// Every backspace burst the emitter has seen, in order — the
    /// shape most correction assertions are made of.
    fn erase_counts(h: &Harness) -> Vec<usize> {
        h.emitter
            .ops()
            .iter()
            .filter_map(|o| match o {
                EmitOp::Backspaces(n) => Some(*n),
                _ => None,
            })
            .collect()
    }

    fn type_word(h: &Harness, scancodes: &[u32]) {
        for &sc in scancodes {
            h.tap(sc);
        }
    }

    /// The real pipeline the app wires up: dictionary first,
    /// word-plausibility second. The domain regressions need it — the
    /// bug they cover only exists against real scoring.
    fn real_detectors() -> Vec<Box<dyn Detector>> {
        use crate::layouts::LayoutDb;
        let layouts = LayoutDb::load_embedded();
        let dicts: std::collections::HashMap<LayoutId, poltertype_detect::LayoutDictionary> =
            layouts
                .iter()
                .filter_map(|(id, m)| m.dictionary.as_ref().map(|d| (id.clone(), d.clone())))
                .collect();
        let profiles = layouts
            .iter()
            .map(|(id, m)| (id.clone(), m.detector_profile()))
            .collect();
        vec![
            Box::new(poltertype_detect::DictionaryDetector::new(dicts)),
            Box::new(poltertype_detect::WordPlausibilityDetector::new(profiles)),
        ]
    }

    /// Which en-US key carries `ch`, and whether it needs Shift.
    fn en_us_key(m: &crate::layouts::LayoutMapping, ch: char) -> (u32, bool) {
        if ch == ' ' {
            return (SPACE, false);
        }
        m.keys
            .iter()
            .find_map(|(&sc, &(plain, shift))| {
                if plain == ch {
                    Some((sc, false))
                } else if shift == Some(ch) {
                    Some((sc, true))
                } else {
                    None
                }
            })
            .unwrap_or_else(|| panic!("no en-US scancode for {ch:?}"))
    }

    /// Type `text` as if on a physical en-US keyboard with the Caps
    /// Lock latch on — the state the listeners report as
    /// `Modifiers::caps`, which is not a held Shift and is applied by
    /// xkb rather than by us.
    fn type_en_us_caps(h: &Harness, text: &str) {
        use crate::layouts::LayoutDb;
        let layouts = LayoutDb::load_embedded();
        let m = layouts.get(&LayoutId::from("en-US")).expect("en-US");
        for ch in text.chars() {
            let (sc, shift) = en_us_key(m, ch);
            let mods = poltertype_types::Modifiers {
                shift,
                caps: true,
                ..poltertype_types::Modifiers::NONE
            };
            h.key_mods(sc, KeyDirection::Press, mods);
            h.key_mods(sc, KeyDirection::Release, mods);
        }
    }

    /// Type `text` as if on a physical en-US keyboard.
    fn type_en_us(h: &Harness, text: &str) {
        use crate::layouts::LayoutDb;
        let layouts = LayoutDb::load_embedded();
        let m = layouts.get(&LayoutId::from("en-US")).expect("en-US");
        for ch in text.chars() {
            let (sc, shift) = if ch == ' ' {
                (SPACE, false)
            } else {
                m.keys
                    .iter()
                    .find_map(|(&sc, &(plain, shift))| {
                        if plain == ch {
                            Some((sc, false))
                        } else if shift == Some(ch) {
                            Some((sc, true))
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| panic!("no en-US scancode for {ch:?}"))
            };
            h.key(sc, KeyDirection::Press, shift);
            h.key(sc, KeyDirection::Release, shift);
        }
    }

    /// Regression: a domain was switched **twice** — once to mangle the
    /// host, then back on the next prose word. `.` is `ю` in uk-UA, so a
    /// host stays one token and its en-US rendering scored 0.00 against
    /// the Cyrillic 0.75.
    #[test]
    fn domain_in_a_sentence_does_not_switch_the_layout() {
        let h = Harness::start_full(
            60_000,
            MockEmitter::default(),
            false,
            None,
            Some(real_detectors()),
        );
        type_en_us(&h, "check games.just-code.net now ");
        h.settle();
        let switches = h.switcher.switches.lock().clone();
        assert!(
            switches.is_empty(),
            "a domain typed in its own layout must not switch anything, got {switches:?}"
        );
        let (ops, _) = h.stop();
        assert!(
            ops.is_empty(),
            "nothing should have been rewritten: {ops:?}"
        );
    }

    /// Issue #33, the residue on 0.21.0: `auto-switch ` typed in
    /// en-US came back as `ФГЕЩ-ЫЦШЕСР `. Both halves of that need
    /// explaining, and this pins the half that needs no Caps Lock —
    /// whether a hyphenated English compound survives the detector at
    /// all. If it does not, the capitals are a second, separate fault
    /// and the correction itself was already wrong.
    #[test]
    fn hyphenated_english_compound_is_not_switched() {
        for word in ["auto-switch ", "wrong-layout ", "cross-platform "] {
            let h = Harness::start_full(
                60_000,
                MockEmitter::default(),
                false,
                None,
                Some(real_detectors()),
            );
            type_en_us(&h, word);
            h.settle();
            let switches = h.switcher.switches.lock().clone();
            assert!(
                switches.is_empty(),
                "{word:?} is English typed in English — nothing to switch, got {switches:?}"
            );
            let (ops, _) = h.stop();
            assert!(ops.is_empty(), "{word:?} should not be rewritten: {ops:?}");
        }
    }

    /// The other half of #33: the same compound typed with Caps Lock
    /// on. The lock is what puts the capitals on screen — we replay
    /// scancodes and let xkb apply it — so the guard that has to hold
    /// is the ALL-CAPS filter, and a hyphen must not let a word slip
    /// past it.
    #[test]
    fn a_hyphenated_word_under_caps_lock_is_left_alone() {
        let h = Harness::start_full(
            60_000,
            MockEmitter::default(),
            false,
            None,
            Some(real_detectors()),
        );
        type_en_us_caps(&h, "auto-switch ");
        h.settle();
        let switches = h.switcher.switches.lock().clone();
        assert!(
            switches.is_empty(),
            "ALL-CAPS text is deliberate spelling-out, hyphen or not, got {switches:?}"
        );
        let (ops, _) = h.stop();
        assert!(
            ops.is_empty(),
            "nothing should have been rewritten: {ops:?}"
        );
    }

    /// The domain guard must not go so wide that it swallows real
    /// corrections: `союз` typed under en-US comes out as `cj.p` — dot
    /// and all — and still has to be fixed.
    #[test]
    fn cyrillic_word_rendering_with_a_dot_is_still_corrected() {
        let h = Harness::start_full(
            60_000,
            MockEmitter::default(),
            false,
            None,
            Some(real_detectors()),
        );
        type_en_us(&h, "cj.p ");
        h.settle();
        assert_eq!(
            *h.switcher.switches.lock(),
            vec![LayoutId::from("uk-UA")],
            "`cj.p` is `союз` mistyped, not a hostname"
        );
    }

    /// Regression: `/tmp ` came back as `/еьз `. The path segment ends
    /// with an ordinary space, so the boundary that says "this is a
    /// path" is the slash *before* it — and nothing was reading that.
    #[test]
    fn path_segment_after_a_slash_is_not_corrected() {
        let h = Harness::start_full(
            60_000,
            MockEmitter::default(),
            false,
            None,
            Some(real_detectors()),
        );
        type_en_us(&h, "cd /tmp ");
        h.settle();
        let switches = h.switcher.switches.lock().clone();
        assert!(
            switches.is_empty(),
            "a path segment must not switch anything, got {switches:?}"
        );
        let (ops, _) = h.stop();
        assert!(
            ops.is_empty(),
            "nothing should have been rewritten: {ops:?}"
        );
    }

    /// The structural prefix must expire at the next separator, or one
    /// slash in a line would disarm the engine for the rest of it.
    #[test]
    fn a_slash_earlier_in_the_line_still_leaves_prose_correctable() {
        let h = Harness::start_full(
            60_000,
            MockEmitter::default(),
            false,
            None,
            Some(real_detectors()),
        );
        type_en_us(&h, "/tmp ghbdsn ");
        h.settle();
        assert_eq!(
            *h.switcher.switches.lock(),
            vec![LayoutId::from("uk-UA")],
            "`ghbdsn` is `привіт` mistyped and opens after a space, not a slash"
        );
    }

    /// Regression: `тех` typed in uk-UA came back as `nt[`. Its en-US
    /// render carries a bracket, and the skeleton left after stripping
    /// it — `nt` — is in `dwyl/english-words`, so the dictionary
    /// detector claimed the word at confidence 0.95.
    #[test]
    fn cyrillic_word_whose_latin_render_is_punctuated_is_left_alone() {
        let h = Harness::start_full(
            60_000,
            MockEmitter::default(),
            false,
            None,
            Some(real_detectors()),
        );
        *h.switcher.current.lock() = LayoutId::from("uk-UA");
        // The physical keys of `тех` are en-US `n`, `t`, `[`.
        type_en_us(&h, "nt[ ");
        h.settle();
        let switches = h.switcher.switches.lock().clone();
        assert!(
            switches.is_empty(),
            "a Cyrillic word whose alt render is punctuated must stay, got {switches:?}"
        );
        let (ops, _) = h.stop();
        assert!(
            ops.is_empty(),
            "nothing should have been rewritten: {ops:?}"
        );
    }

    /// Regression: `command --wsl ` came back as `command --цід `. The
    /// hyphen is a word character, so a flag reaches the detectors as
    /// one token with no separator any earlier filter can see. `wsl`
    /// is in the shell vocabulary now, which covers that flag twice
    /// over — so the flag here is one the dictionary *would* claim,
    /// which is what the guard itself has to answer for.
    #[test]
    fn command_line_flag_is_not_corrected() {
        let h = Harness::start_full(
            60_000,
            MockEmitter::default(),
            false,
            None,
            Some(real_detectors()),
        );
        type_en_us(&h, "command --ghbdsn ");
        h.settle();
        let switches = h.switcher.switches.lock().clone();
        assert!(
            switches.is_empty(),
            "a command-line flag must not switch anything, got {switches:?}"
        );
        let (ops, _) = h.stop();
        assert!(
            ops.is_empty(),
            "nothing should have been rewritten: {ops:?}"
        );
    }

    /// Regression: rubbing a line out with Backspace left the engine
    /// mute — every word retyped afterwards was reported "tainted" and
    /// never corrected. Deleting past what the buffer tracks says the
    /// *context* is unknown, not the word typed next.
    #[test]
    fn word_typed_after_a_rubbed_out_line_is_still_corrected() {
        let h = Harness::start_full(
            60_000,
            MockEmitter::default(),
            false,
            None,
            Some(real_detectors()),
        );
        type_en_us(&h, "hello ");
        h.settle();
        assert!(
            h.switcher.switches.lock().is_empty(),
            "`hello` is English — nothing to correct"
        );
        // Past the space, past the word, past everything ever tracked.
        for _ in 0..9 {
            h.tap(BACKSPACE);
        }
        type_en_us(&h, "ghbdsn ");
        h.settle();
        assert_eq!(
            *h.switcher.switches.lock(),
            vec![LayoutId::from("uk-UA")],
            "`ghbdsn` is `привіт` mistyped, deletions before it or not"
        );
    }

    /// Baseline ordering: switch first, then word-length+boundary
    /// backspaces, then the scancode replay ending in the boundary.
    #[test]
    fn basic_correction_switches_then_deletes_then_replays() {
        let h = Harness::start(60_000);
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.settle();
        assert_eq!(
            *h.switcher.switches.lock(),
            vec![LayoutId::from("uk-UA")],
            "layout must switch exactly once, to the detector's pick"
        );
        let (ops, _) = h.stop();
        assert_eq!(
            ops,
            vec![
                EmitOp::Backspaces(7),
                EmitOp::Keys(GHBDSN.iter().copied().chain([SPACE]).collect()),
            ]
        );
    }

    /// The separator that closed a word must survive the correction as
    /// the character the user saw. Reported as: `Photos` then `,` under
    /// uk-UA came out `Photos?`, the boundary key having been replayed
    /// by scancode against the *new* layout.
    ///
    /// The reported key was `Shift`+`0x35`, but this harness loads all
    /// fifteen bundled layouts and bg-BG carries a letter there, which
    /// makes it a word key rather than a boundary. Hence the same trap
    /// one row up: `Shift`+`0x08` is `?` under uk-UA and `&` under
    /// en-US, and `?` lives on `Shift`+`0x35` in en-US.
    #[test]
    fn boundary_character_survives_the_layout_flip() {
        let h = Harness::start(60_000);
        *h.switcher.current.lock() = LayoutId::from("uk-UA");
        type_word(&h, &GHBDSN);
        h.key(0x08, KeyDirection::Press, true);
        h.key(0x08, KeyDirection::Release, true);
        h.settle();
        assert_eq!(
            *h.switcher.switches.lock(),
            vec![LayoutId::from("en-US")],
            "the word itself still has to be corrected"
        );
        let replays = h.emitter.replays.lock().clone();
        let last = replays.last().expect("a replay burst").clone();
        assert_eq!(
            last.last().copied(),
            Some((0x35, true)),
            "the `?` the user typed must be re-emitted on the key that \
             produces `?` under en-US, not on the one they pressed: {last:?}"
        );
    }

    /// A word typed under a latched Caps Lock has to go back out on the
    /// Shift states the user's fingers actually had.
    ///
    /// xkb applies the lock a second time to whatever we emit: press
    /// Shift for a capital the lock produced and the letter comes back
    /// lower-case, while a digit or a punctuation mark — which the lock
    /// never touched — comes back as its shifted symbol. Reported as
    /// "capital letters are incorrect and sometimes random symbols
    /// incorrectly changed"
    /// ([#33](https://github.com/Just-Code-NET/PolterType/issues/33)).
    /// Driven through the manual switch-last hotkey because that is the
    /// path a locked keyboard actually reaches: rendered under the lock
    /// the word reads ALL CAPS, and the automatic path hands those to
    /// the abbreviation guard.
    #[test]
    fn a_word_typed_under_caps_lock_replays_without_shift() {
        let h = Harness::start(60_000);
        let caps = poltertype_types::Modifiers {
            caps: true,
            ..poltertype_types::Modifiers::NONE
        };
        for &sc in GHBDSN.iter().chain(std::iter::once(&SPACE)) {
            h.key_mods(sc, KeyDirection::Press, caps);
            h.key_mods(sc, KeyDirection::Release, caps);
        }
        h.settle();
        h.cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        h.settle();
        let replays = h.emitter.replays.lock().clone();
        let last = replays.last().expect("a replay burst").clone();
        assert!(
            last.iter().all(|&(_, shift)| !shift),
            "the user pressed no Shift, so the replay must press none — \
             the lock capitalises the letters on its own: {last:?}"
        );
    }

    /// A switch that reports success and is then put back must not cost
    /// the user their word.
    ///
    /// Measured on MATE, 2026-08-24: `XkbLatchLockState` returns fine,
    /// `mate-settings-daemon` restores its own group within
    /// milliseconds, and the correction went ahead regardless —
    /// deleting five keystrokes and retyping the same five. Doing
    /// nothing is strictly better than that.
    #[test]
    fn a_layout_switch_that_gets_put_back_leaves_the_word_alone() {
        let h = Harness::start(60_000);
        *h.switcher.revert.lock() = true;
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.settle();
        assert_eq!(
            *h.switcher.switches.lock(),
            vec![LayoutId::from("uk-UA")],
            "the switch is still attempted — it is the retype that must not follow"
        );
        let (ops, _) = h.stop();
        assert!(
            ops.is_empty(),
            "nothing may be deleted or retyped once the layout went back: {ops:?}"
        );
    }

    /// A desktop that switches when asked never sees a keystroke from
    /// us.
    ///
    /// The chord exists for GNOME 49 and MATE, which accept nothing
    /// else — but sending it where the direct switch worked would put a
    /// stray `Super+space` into the user's session for no reason, and
    /// on a desktop with three layouts it would land on the wrong one.
    #[test]
    fn a_desktop_that_switches_properly_is_never_sent_a_shortcut() {
        let h = Harness::start(60_000);
        *h.switcher.chord.lock() = Some(poltertype_types::SwitchChord {
            scancode: 0x39,
            meta: true,
            ..Default::default()
        });
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.settle();
        assert!(
            h.emitter.chords.lock().is_empty(),
            "the layout already moved; pressing the desktop's shortcut would move it again"
        );
        let (ops, _) = h.stop();
        assert!(!ops.is_empty(), "and the correction still happened");
    }

    /// When the desktop puts the layout back *and* its shortcut does
    /// not help either, the word is still left alone — the shortcut is
    /// tried, not trusted.
    #[test]
    fn a_shortcut_that_does_not_help_still_costs_the_user_nothing() {
        let h = Harness::start(60_000);
        *h.switcher.revert.lock() = true;
        *h.switcher.chord.lock() = Some(poltertype_types::SwitchChord {
            scancode: 0x2A,
            alt: true,
            ..Default::default()
        });
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.settle();
        let chords = h.emitter.chords.lock().len();
        assert!(
            chords > 0 && chords <= 2,
            "the shortcut is tried once per layout and no more, got {chords}"
        );
        let (ops, _) = h.stop();
        assert!(
            ops.is_empty(),
            "nothing may be deleted or retyped when the layout never moved: {ops:?}"
        );
    }

    /// Switching the layout by hand between a word and the key that
    /// closes it must not make the engine "correct" text that is already
    /// right. Reported as: type `Photos` in en-US, switch to uk-UA,
    /// press `,` — and the whole word is retyped.
    #[test]
    fn manual_switch_before_the_boundary_suppresses_the_correction() {
        let h = Harness::start(60_000);
        type_word(&h, &GHBDSN);
        // The engine must see the word's first key before the layout
        // moves, or it stamps the word with the new layout and there is
        // nothing to notice.
        h.settle();
        *h.switcher.current.lock() = LayoutId::from("uk-UA");
        h.tap(SPACE);
        h.settle();
        assert!(
            h.switcher.switches.lock().is_empty(),
            "the user's own choice of layout must stand"
        );
        let (ops, _) = h.stop();
        assert!(ops.is_empty(), "nothing should have been retyped: {ops:?}");
    }

    /// If the layout switch fails, the correction must abort BEFORE any
    /// backspace reaches the user's text — deleting first and then
    /// discovering the switch is impossible destroys the word.
    #[test]
    fn failed_switch_leaves_text_untouched() {
        let h = Harness::start_with(60_000, MockEmitter::default(), true);
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.settle();
        let (ops, _) = h.stop();
        assert_eq!(
            ops,
            vec![],
            "no keystrokes may be sent if the switch failed"
        );
    }

    /// Echo immunity: feeding the correction's own keystrokes back
    /// (what keyd does) must not trigger another correction or leave
    /// junk in the buffer that breaks the next word.
    #[test]
    fn echoes_do_not_retrigger_or_pollute() {
        let h = Harness::start(60_000);
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        // Echoes arrive one keyd round-trip later, while the engine is
        // still inside its post-replay settle window.
        h.wait_ops(2);
        h.replay_echoes();
        h.settle();
        assert_eq!(h.emitter.ops().len(), 2, "echoes must not re-correct");

        // Buffer unpolluted: the next mistyped word corrects with the
        // right backspace count (its own length + boundary — not more).
        type_word(&h, &GHBDSN); // now typed under uk-UA → detector → en-US
        h.tap(SPACE);
        h.settle();
        let (ops, _) = h.stop();
        assert_eq!(ops[2], EmitOp::Backspaces(7));
    }

    /// Reported symptom "word chopped in half": complete a word,
    /// backspace over the space and two letters, retype them, complete
    /// again. The second correction must cover the WHOLE word (7
    /// backspaces), not just the retyped tail (3).
    #[test]
    fn backspace_edit_recorrects_whole_word() {
        let h = Harness::start(60_000);
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.wait_ops(2);
        h.replay_echoes(); // keyd delivers our correction's echoes
        h.settle();

        h.tap(BACKSPACE);
        h.tap(BACKSPACE);
        h.tap(GHBDSN[5]);
        h.tap(SPACE);
        h.settle();

        let (ops, _) = h.stop();
        assert_eq!(
            ops.get(2),
            Some(&EmitOp::Backspaces(7)),
            "re-opened word must be corrected in full, got {ops:?}"
        );
        assert_eq!(
            ops.get(3),
            Some(&EmitOp::Keys(
                GHBDSN.iter().copied().chain([SPACE]).collect()
            )),
        );
    }

    /// Reported symptom "typing through a correction": the raced
    /// keystroke is absorbed into the plan before anything is deleted —
    /// one extra backspace, re-typed after the boundary, and seeded into
    /// the next word's buffer.
    #[test]
    fn raced_keystroke_is_compensated() {
        let h = Harness::start(60_000);
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        // Deterministic: the engine watches the channel for a quiet gap
        // before deleting, so this letter is always already in flight.
        h.press(GHBDSN[0]);
        h.release(GHBDSN[0]);
        h.settle();

        let ops = h.emitter.ops();
        assert_eq!(
            ops[0],
            EmitOp::Backspaces(8),
            "single burst covers word + boundary + absorbed key, got {ops:?}"
        );
        let EmitOp::Keys(replayed) = &ops[1] else {
            panic!("expected replay op, got {ops:?}");
        };
        assert_eq!(
            replayed.last(),
            Some(&GHBDSN[0]),
            "raced key must be re-typed after the boundary"
        );

        // Finish the word with 5 more letters: the next correction must
        // count all 6 + boundary.
        for &sc in &GHBDSN[1..] {
            h.tap(sc);
        }
        h.tap(SPACE);
        h.settle();
        let (ops, _) = h.stop();
        assert_eq!(
            ops.get(2),
            Some(&EmitOp::Backspaces(7)),
            "raced key must be part of the next tracked word, got {ops:?}"
        );
    }

    /// The full fast-typing race: the user types the second word and its
    /// boundary before the first correction begins. Everything must come
    /// out in order, and word2 must get its own decision.
    #[test]
    fn raced_full_word_is_absorbed_in_order() {
        let h = Harness::start(60_000);
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        type_word(&h, &GHBDSN); // entire second word already queued
        h.tap(SPACE);
        h.settle();

        let ops = h.emitter.ops();
        // Correction 1 absorbs word2 up to its boundary:
        // word1(6) + space(1) + word2(6) + space(1) = 14.
        assert_eq!(
            ops[0],
            EmitOp::Backspaces(14),
            "must absorb the raced word + its boundary, got {ops:?}"
        );
        let expected_replay: Vec<u32> = GHBDSN
            .iter()
            .copied()
            .chain([SPACE])
            .chain(GHBDSN.iter().copied())
            .chain([SPACE])
            .collect();
        assert_eq!(
            ops[1],
            EmitOp::Keys(expected_replay),
            "replay must preserve typed order, got {ops:?}"
        );
        // The resume boundary routed word2 through the normal pipeline,
        // where the flip-flop mock detector corrects it in its own right
        // (7 = 6 keys + boundary).
        assert_eq!(
            ops.get(2),
            Some(&EmitOp::Backspaces(7)),
            "absorbed word must get its own decision, got {ops:?}"
        );
        let (_, events) = h.stop();
        assert!(
            events
                .iter()
                .filter(|e| matches!(e, SwitcherEvent::Corrected { .. }))
                .count()
                >= 2,
            "both words corrected: {events:?}"
        );
    }

    /// A key that appears nowhere in the correction being replayed: an
    /// intruder sharing a scancode with our own replay is swallowed by
    /// the echo queue instead, which makes these tests depend on how
    /// fast the echoes happen to arrive.
    const INTRUDER: u32 = 0x2D; // `X` — not in GHBDSN, not SPACE

    /// Send one press+release of `sc` into the engine's key stream from
    /// wherever it is called — a keystroke the compositor interleaves
    /// with a burst we are still emitting.
    fn intrude(key_tx: &Sender<KeyEvent>, sc: u32) {
        for direction in [KeyDirection::Press, KeyDirection::Release] {
            let _ = key_tx.send(KeyEvent {
                vk: sc,
                scancode: sc,
                direction,
                modifiers: poltertype_types::Modifiers::NONE,
                injected: false,
                timestamp_ms: 0,
            });
        }
    }

    /// The next word's first key reaches the compositor mid-replay and
    /// lands among our own characters (`зтзь ш ` → `ipnpm `). Nothing in
    /// the key stream says where, so the engine erases everything it
    /// typed, the intruder included, and re-emits in typed order.
    #[test]
    fn keystroke_inside_the_replay_is_repaired() {
        let h = Harness::start(60_000);
        let key_tx = h.key_tx.clone();
        let fired = Arc::new(Mutex::new(false));
        {
            let fired = Arc::clone(&fired);
            *h.emitter.during_replay.lock() = Some(Box::new(move || {
                // Only the first burst gets raced: the repair must then
                // succeed and settle.
                if std::mem::replace(&mut *fired.lock(), true) {
                    return;
                }
                intrude(&key_tx, INTRUDER);
            }));
        }
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.settle();

        let (ops, _) = h.stop();
        let word: Vec<u32> = GHBDSN.iter().copied().chain([SPACE]).collect();
        let repaired: Vec<u32> = word.iter().copied().chain([INTRUDER]).collect();
        assert_eq!(
            ops,
            vec![
                EmitOp::Backspaces(7),
                EmitOp::Keys(word),
                // The 7 characters we put on screen plus the one that
                // got in among them.
                EmitOp::Backspaces(8),
                EmitOp::Keys(repaired),
            ],
            "an intruding keystroke must trigger a re-emit in typed order"
        );
    }

    /// The repair is budgeted. A user who keeps landing keys inside
    /// every burst must not put the engine in an emit loop over their
    /// text — it gives up and leaves the screen alone instead.
    #[test]
    fn relentless_intrusion_stops_at_the_repair_budget() {
        let h = Harness::start(60_000);
        let key_tx = h.key_tx.clone();
        *h.emitter.during_replay.lock() = Some(Box::new(move || {
            intrude(&key_tx, INTRUDER);
        }));
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.settle();

        let (ops, _) = h.stop();
        let replays = ops.iter().filter(|o| matches!(o, EmitOp::Keys(_))).count();
        assert_eq!(
            replays,
            1 + INTRUSION_REPAIRS,
            "one replay plus the repair budget, then stop, got {ops:?}"
        );
    }

    /// A correction fired by a chord starts while that chord's modifiers
    /// are still down, and the replay reaches the application the way
    /// the user's keys do — so under a held Ctrl every replayed key
    /// arrives as a shortcut and nothing is typed.
    #[test]
    fn accept_chord_releases_its_own_modifiers_before_typing() {
        // `Ctrl+Meta` also exercises parsing `Meta` — the half the
        // default `Ctrl+Shift` never touches.
        let h = suggestion_harness_with_chord(Some("Ctrl+Meta"));
        type_word(&h, &HWLLO);
        h.tap(SPACE);
        let _generation = ready_generation(&h);
        let chord = poltertype_types::Modifiers {
            control: true,
            meta: true,
            ..poltertype_types::Modifiers::NONE
        };
        h.key_mods(0x1D, KeyDirection::Press, chord);
        h.key_mods(0x7D, KeyDirection::Press, chord);
        h.key_mods(0x02, KeyDirection::Press, chord);
        h.settle();

        let (ops, _) = h.stop();
        assert_eq!(
            ops.first(),
            Some(&EmitOp::ReleaseModifiers),
            "the chord's modifiers must be let go before anything is typed, got {ops:?}"
        );
        assert!(
            ops.iter().any(|o| matches!(o, EmitOp::Keys(_))),
            "and the replacement must still be typed, got {ops:?}"
        );
    }

    /// The common case must not pay for it: no modifiers held, no
    /// release burst — those are keystrokes too, and every one of them
    /// widens the window a user keystroke can land in.
    #[test]
    fn plain_correction_does_not_release_modifiers() {
        let h = Harness::start(60_000);
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.settle();

        let (ops, _) = h.stop();
        assert!(
            !ops.contains(&EmitOp::ReleaseModifiers),
            "nothing was held, so nothing should be released, got {ops:?}"
        );
    }

    /// Arrow keys mid-word poison the word: no correction may fire on
    /// a word the buffer only partially observed.
    #[test]
    fn nav_mid_word_suppresses_correction() {
        let h = Harness::start(60_000);
        type_word(&h, &GHBDSN[..3]);
        h.tap(105); // KEY_LEFT
        type_word(&h, &GHBDSN[3..]);
        h.tap(SPACE);
        h.settle();
        let (ops, events) = h.stop();
        assert_eq!(ops, vec![], "tainted word must not be corrected");
        assert!(
            events.iter().any(|e| matches!(
                e,
                SwitcherEvent::KeptCurrent { reason } if reason.contains("lost track")
            )),
            "engine should report why it stayed quiet: {events:?}"
        );
    }

    /// An idle pause mid-word must not let the engine correct only the
    /// tail it saw afterwards, leaving the word's head behind.
    #[test]
    fn idle_gap_mid_word_suppresses_correction() {
        let h = Harness::start(50); // 50 ms idle timeout
        type_word(&h, &GHBDSN[..3]);
        h.settle();
        std::thread::sleep(Duration::from_millis(120));
        type_word(&h, &GHBDSN[3..]);
        h.tap(SPACE);
        h.settle();
        let (ops, _) = h.stop();
        assert_eq!(
            ops,
            vec![],
            "word interrupted by an idle gap must not be corrected"
        );
    }

    /// A mouse click mid-word means the caret may have landed inside
    /// the word being typed — correcting what we saw afterwards would
    /// splice layouts mid-word. Must stay quiet.
    #[test]
    fn click_mid_word_suppresses_correction() {
        let h = Harness::start(60_000);
        type_word(&h, &GHBDSN[..3]);
        h.press(poltertype_types::SC_POINTER_BUTTON); // click somewhere
        type_word(&h, &GHBDSN[3..]);
        h.tap(SPACE);
        h.settle();
        let (ops, _) = h.stop();
        assert_eq!(
            ops,
            vec![],
            "word interrupted by a click must not be corrected"
        );
    }

    /// The main chat-box flow: click into an input field, type a word in
    /// the wrong layout, hit space. A click must not cost the user their
    /// next correction, and the count must be exactly the word's length.
    #[test]
    fn click_then_fresh_word_corrects_normally() {
        let h = Harness::start(60_000);
        h.press(poltertype_types::SC_POINTER_BUTTON); // click into a field
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.settle();
        let (ops, _) = h.stop();
        assert_eq!(
            ops,
            vec![
                EmitOp::Backspaces(7),
                EmitOp::Keys(GHBDSN.iter().copied().chain([SPACE]).collect()),
            ],
            "the word after a click must correct with exactly its own length"
        );
    }

    // ─── Selection conversion ────────────────────────────────────────

    /// A harness whose force-switch has no word to act on and a
    /// clipboard to fall back to, with the focused application's half
    /// of a copy wired to the emitter: press copy, and `selection`
    /// appears on the clipboard, exactly as an application answers
    /// `Ctrl+C`.
    fn selection_harness(selection: &str) -> (Harness, Arc<MockClipboard>) {
        let clipboard = Arc::new(MockClipboard::default());
        let emitter = MockEmitter::default();
        let cb = Arc::clone(&clipboard);
        let text = selection.to_owned();
        *emitter.on_chord.lock() = Some(Box::new(move |chord| {
            if chord == crate::engine::consts::COPY_CHORD {
                use poltertype_input::Clipboard as _;
                let _ = cb.set_text(&text);
            }
        }));
        let h = Harness::start_configured(
            60_000,
            emitter,
            false,
            None,
            None,
            None,
            |s| s.selection.enabled = true,
            Some(Arc::clone(&clipboard) as Arc<dyn poltertype_input::Clipboard>),
        );
        (h, clipboard)
    }

    /// The feature's own shape, and its first end-to-end test: nothing
    /// to switch, so the hotkey copies, converts and pastes — then puts
    /// the user's clipboard back.
    #[test]
    fn a_selection_is_converted_and_the_clipboard_restored() {
        // `ghbdsn` is `привіт` typed under en-US.
        let (h, clipboard) = selection_harness("ghbdsn");
        {
            use poltertype_input::Clipboard as _;
            clipboard
                .set_text("something the user copied earlier")
                .expect("mock clipboard");
        }

        h.cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        h.settle();

        let writes = clipboard.writes();
        assert!(
            writes.iter().any(|w| w == "привіт"),
            "the converted text must be staged for the paste: {writes:?}"
        );
        assert_eq!(
            writes.last().map(String::as_str),
            Some("something the user copied earlier"),
            "and the user's own clipboard must be the last thing put back: {writes:?}"
        );
    }

    /// Issue #51, the half that only X11 shows: while the chord that
    /// asked for the conversion is still held, an `XGrabKey` passive
    /// grab is an *active* one and everything we emit goes to the
    /// client holding it — so the copy chord never reaches the window
    /// the selection is in, and the gesture reads as "nothing was
    /// selected". Every other force-switch path already waits for the
    /// key to come up; this one did not.
    #[test]
    fn a_selection_is_left_alone_until_the_hotkey_comes_up() {
        const KEY: u32 = 0x43; // F9
        let (h, clipboard) = selection_harness("ghbdsn");
        h.cmd_tx
            .send(EngineCommand::SetKeystreamHotkeys(KeystreamHotkeys {
                // *Grabbed*, which is what makes the key invisible to
                // us on X11 and is how `trigger_key_down` reads it.
                grabbed: [
                    Some(Chord {
                        ctrl: true,
                        shift: true,
                        alt: false,
                        meta: false,
                        scancode: KEY,
                    }),
                    None,
                ],
                pause: None,
                switch_last: None,
            }))
            .expect("engine alive");

        let both = poltertype_types::Modifiers {
            control: true,
            shift: true,
            ..poltertype_types::Modifiers::NONE
        };
        h.key_mods(0x1D, KeyDirection::Press, both);
        h.key_mods(0x2A, KeyDirection::Press, both);
        // Before the command: the run loop takes keys and commands off
        // two channels, and the modifiers have to be on record as held
        // by the time the force-switch is handled.
        h.settle();
        h.cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        std::thread::sleep(Duration::from_millis(150));
        assert!(
            clipboard.writes().is_empty(),
            "nothing may be emitted at the clipboard while the chord is down: {:?}",
            clipboard.writes()
        );

        // Finger off, and the same press finishes its work.
        h.key_mods(
            0x2A,
            KeyDirection::Release,
            poltertype_types::Modifiers::NONE,
        );
        h.key_mods(
            0x1D,
            KeyDirection::Release,
            poltertype_types::Modifiers::NONE,
        );
        h.settle();
        assert!(
            clipboard.writes().iter().any(|w| w == "привіт"),
            "and once it is up, the selection converts: {:?}",
            clipboard.writes()
        );
    }

    /// Issue #57: an arrow key, then a whole word typed after it, then
    /// the manual hotkey — and nothing happened.
    ///
    /// `abandon` poisons the buffer so the *interrupted* word is never
    /// corrected from the tail it saw. Nav clears the buffer with it,
    /// so the poison could only ever reach the *next* word — one
    /// watched from its first key, at a caret that is exactly where we
    /// put it. `word_in_progress` refuses a poisoned buffer, so the
    /// gesture stayed dead until a separator was typed. Same shape as
    /// issues #40 and #44, one trigger further along.
    #[test]
    fn the_manual_hotkey_reaches_a_word_typed_after_a_caret_move() {
        let h = Harness::start(60_000);
        type_word(&h, &GHBDSN[..3]);
        h.tap(105); // KEY_LEFT — the caret goes somewhere we can't see
        type_word(&h, &GHBDSN);
        h.settle();

        h.cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        h.settle();
        assert_eq!(
            erase_counts(&h),
            vec![GHBDSN.len()],
            "the hotkey must convert the word typed after the arrow: {:?}",
            h.emitter.ops()
        );
    }

    /// Issue #56: after a manual switch, clicking away and back left
    /// the hotkey dead for the next word.
    ///
    /// The switched word stays in the buffer (`settle`), so the click
    /// that leaves the window abandons a non-empty buffer and poisons
    /// it — and the poison outlived the click back, the new window and
    /// a word typed entirely under our own eyes.
    #[test]
    fn the_manual_hotkey_survives_leaving_a_window_and_coming_back() {
        let h = Harness::start(60_000);
        type_word(&h, &GHBDSN);
        h.settle();
        h.cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        h.settle();
        assert_eq!(
            erase_counts(&h),
            vec![GHBDSN.len()],
            "precondition: the first press converts the word in progress"
        );

        h.press(poltertype_types::SC_POINTER_BUTTON); // click another window
        h.press(poltertype_types::SC_POINTER_BUTTON); // and click back
        type_word(&h, &GHBDSN);
        h.settle();
        h.cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        h.settle();
        assert_eq!(
            erase_counts(&h),
            vec![GHBDSN.len(), GHBDSN.len()],
            "the second press must convert the word typed after the clicks: {:?}",
            h.emitter.ops()
        );
    }

    // ─── Spelling suggestions ────────────────────────────────────────

    /// Leaves every word as typed, so the suggestions gate is reached on
    /// each completed word.
    struct NoOpinionDetector;

    impl Detector for NoOpinionDetector {
        fn name(&self) -> &'static str {
            "test-no-opinion"
        }
        fn judge(&self, _ctx: &poltertype_detect::DetectionContext<'_>) -> Verdict {
            Verdict::NoOpinion
        }
    }

    /// Like `AlwaysOther`, but too unsure to clear the 0.55 threshold
    /// — the verdict must surface as the leading tooltip entry
    /// instead of an auto-switch.
    struct TimidOther(LayoutId, LayoutId);

    impl Detector for TimidOther {
        fn name(&self) -> &'static str {
            "test-timid-other"
        }
        fn judge(&self, ctx: &poltertype_detect::DetectionContext<'_>) -> Verdict {
            let target = if *ctx.current_layout == self.0 {
                self.1.clone()
            } else {
                self.0.clone()
            };
            Verdict::Switch(DetectionVerdict {
                best_layout: target,
                confidence: 0.30,
                reason: "test-low-confidence".into(),
            })
        }
    }

    /// Deterministic provider: every token is "unknown" and maps to a
    /// fixed candidate list.
    struct FixedSuggestions(Vec<&'static str>);

    impl poltertype_detect::SuggestionProvider for FixedSuggestions {
        fn is_known(&self, _layout: &LayoutId, _typed: &str) -> bool {
            false
        }
        fn suggest(
            &self,
            _layout: &LayoutId,
            _typed: &str,
            max: usize,
        ) -> Vec<poltertype_detect::Suggestion> {
            self.0
                .iter()
                .take(max)
                .map(|s| poltertype_detect::Suggestion {
                    text: (*s).to_owned(),
                    score: 0.5,
                })
                .collect()
        }
    }

    /// Answers `is_known` from a fixed `(layout, word)` list — the only
    /// dictionary state the undo-learning guard reads.
    struct KnownWords(&'static [(&'static str, &'static str)]);

    impl poltertype_detect::SuggestionProvider for KnownWords {
        fn is_known(&self, layout: &LayoutId, typed: &str) -> bool {
            let typed = poltertype_detect::letters_only_lower(typed);
            self.0
                .iter()
                .any(|(l, w)| layout.as_str() == *l && typed == *w)
        }
        fn suggest(
            &self,
            _layout: &LayoutId,
            _typed: &str,
            _max: usize,
        ) -> Vec<poltertype_detect::Suggestion> {
            Vec::new()
        }
    }

    fn suggestion_harness() -> Harness {
        suggestion_harness_with_chord(None)
    }

    fn suggestion_harness_with_chord(accept_modifiers: Option<&str>) -> Harness {
        Harness::start_tuned(
            60_000,
            MockEmitter::default(),
            false,
            Some(Arc::new(FixedSuggestions(vec!["hello"]))),
            Some(vec![Box::new(NoOpinionDetector)]),
            accept_modifiers,
        )
    }

    /// `hwllo` / `hello` under en-US.
    const HWLLO: [u32; 5] = [0x23, 0x11, 0x26, 0x26, 0x18];
    const HELLO: [u32; 5] = [0x23, 0x12, 0x26, 0x26, 0x18];

    fn ready_generation(h: &Harness) -> u64 {
        match h.wait_for(|e| matches!(e, SwitcherEvent::SuggestionsReady { .. })) {
            SwitcherEvent::SuggestionsReady { generation, .. } => generation,
            _ => unreachable!(),
        }
    }

    #[test]
    fn mistyped_word_yields_offer_without_touching_text() {
        let h = suggestion_harness();
        type_word(&h, &HWLLO);
        h.tap(SPACE);
        let ev = h.wait_for(|e| matches!(e, SwitcherEvent::SuggestionsReady { .. }));
        let SwitcherEvent::SuggestionsReady {
            original, entries, ..
        } = ev
        else {
            unreachable!()
        };
        assert_eq!(original, "hwllo");
        assert_eq!(
            entries.len(),
            2,
            "one suggestion + the add-to-dictionary row"
        );
        assert_eq!(entries[0].text, "hello");
        assert!(entries[0].switch_to.is_none());
        assert_eq!(entries[0].action, SuggestionAction::Replace);
        // The escape hatch closes the list, carrying the typed word so
        // the accept path knows what to add.
        assert_eq!(entries[1].action, SuggestionAction::AddToDictionary);
        assert_eq!(entries[1].text, "hwllo");
        let (ops, _) = h.stop();
        assert!(ops.is_empty(), "an offer alone must not emit keystrokes");
    }

    #[test]
    fn add_to_dictionary_entry_emits_event_and_no_keystrokes() {
        let h = suggestion_harness();
        type_word(&h, &HWLLO);
        h.tap(SPACE);
        let ev = h.wait_for(|e| matches!(e, SwitcherEvent::SuggestionsReady { .. }));
        let SwitcherEvent::SuggestionsReady {
            generation,
            entries,
            ..
        } = ev
        else {
            unreachable!()
        };
        let add_index = entries
            .iter()
            .position(|e| e.action == SuggestionAction::AddToDictionary)
            .expect("add-to-dictionary row present");
        h.cmd_tx
            .send(EngineCommand::AcceptSuggestion {
                generation,
                index: add_index,
                typed_digit: false,
                from_pointer: true,
            })
            .expect("engine alive");
        let ev = h.wait_for(|e| matches!(e, SwitcherEvent::AddToDictionary { .. }));
        let SwitcherEvent::AddToDictionary {
            layout,
            word,
            origin,
        } = ev
        else {
            unreachable!()
        };
        assert_eq!(layout, LayoutId::from("en-US"));
        assert_eq!(word, "hwllo");
        assert_eq!(origin, DictionaryAddOrigin::Tooltip);
        let (ops, _) = h.stop();
        assert!(
            ops.is_empty(),
            "adding to the dictionary must not type anything"
        );
    }

    /// A word that starts right after a click may be a fragment of a
    /// longer on-screen word — no tooltip for it. The next word,
    /// started after an observed separator, gets one again.
    #[test]
    fn unclean_word_start_suppresses_the_offer() {
        let h = suggestion_harness();
        h.press(poltertype_types::SC_POINTER_BUTTON); // click into text
        h.release(poltertype_types::SC_POINTER_BUTTON);
        type_word(&h, &HWLLO);
        h.tap(SPACE); // completes, but started unclean
        type_word(&h, &HWLLO);
        h.tap(SPACE); // boundary-started — offer expected
        let ev = h.wait_for(|e| matches!(e, SwitcherEvent::SuggestionsReady { .. }));
        let SwitcherEvent::SuggestionsReady { generation, .. } = ev else {
            unreachable!()
        };
        assert_eq!(
            generation, 1,
            "exactly one offer: the click-started word must have stayed quiet"
        );
        let (ops, _) = h.stop();
        assert!(ops.is_empty());
    }

    #[test]
    fn accept_command_replaces_word_in_place() {
        let h = suggestion_harness();
        type_word(&h, &HWLLO);
        h.tap(SPACE);
        let generation = ready_generation(&h);
        h.cmd_tx
            .send(EngineCommand::AcceptSuggestion {
                generation,
                index: 0,
                typed_digit: false,
                from_pointer: false,
            })
            .expect("engine alive");
        h.settle();
        assert!(
            h.switcher.switches.lock().is_empty(),
            "same-layout replacement must not switch layouts"
        );
        let (ops, events) = h.stop();
        assert_eq!(
            ops,
            vec![
                EmitOp::Backspaces(6),
                EmitOp::Keys(HELLO.iter().copied().chain([SPACE]).collect()),
            ],
            "delete word+boundary, retype suggestion scancodes + boundary"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SwitcherEvent::SuggestionApplied { .. })),
            "expected a SuggestionApplied event"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, SwitcherEvent::Corrected { .. })),
            "a same-layout replacement is not a layout correction"
        );
    }

    #[test]
    fn accept_digit_chord_replaces_word() {
        let h = suggestion_harness();
        type_word(&h, &HWLLO);
        h.tap(SPACE);
        let _generation = ready_generation(&h);
        let chord = poltertype_types::Modifiers {
            control: true,
            shift: true,
            ..poltertype_types::Modifiers::NONE
        };
        h.key_mods(0x02, KeyDirection::Press, chord); // Ctrl+Shift+1
        h.key_mods(0x02, KeyDirection::Release, chord);
        h.settle();
        let (ops, _) = h.stop();
        assert_eq!(
            ops,
            vec![
                // The chord's own Ctrl+Shift are still down; typing
                // under them would produce shortcuts, not text.
                EmitOp::ReleaseModifiers,
                // 5 word + 1 boundary + the chord's own digit, which
                // the application received on its way past us.
                EmitOp::Backspaces(7),
                EmitOp::Keys(HELLO.iter().copied().chain([SPACE]).collect()),
            ]
        );
    }

    /// A tooltip click reaches the engine twice: as the physical
    /// `SC_POINTER_BUTTON` press (which abandons the buffer) and as the
    /// popup's `Accepted` command. The click never reached the app
    /// below, so the frozen screen state must still authorise it.
    #[test]
    fn click_accept_survives_pointer_abandon() {
        let h = suggestion_harness();
        type_word(&h, &HWLLO);
        h.tap(SPACE);
        let generation = ready_generation(&h);
        // Physical click observed first…
        h.press(poltertype_types::SC_POINTER_BUTTON);
        h.release(poltertype_types::SC_POINTER_BUTTON);
        std::thread::sleep(Duration::from_millis(60));
        // …the tooltip's Accepted event arrives a beat later.
        h.cmd_tx
            .send(EngineCommand::AcceptSuggestion {
                generation,
                index: 0,
                typed_digit: false,
                from_pointer: true,
            })
            .expect("engine alive");
        h.settle();
        let (ops, _) = h.stop();
        assert_eq!(
            ops,
            vec![
                EmitOp::Backspaces(6),
                EmitOp::Keys(HELLO.iter().copied().chain([SPACE]).collect()),
            ],
            "a tooltip click must replace the word despite its own pointer-abandon"
        );
    }

    /// The other ordering of the same race: the popup's `Accepted`
    /// command wins, and the physical click's key-stream observation
    /// lands while the correction is already absorbing. The allowance
    /// must swallow it instead of aborting as "caret moved".
    #[test]
    fn click_accept_tolerates_click_racing_the_correction() {
        let h = suggestion_harness();
        type_word(&h, &HWLLO);
        h.tap(SPACE);
        let generation = ready_generation(&h);
        h.cmd_tx
            .send(EngineCommand::AcceptSuggestion {
                generation,
                index: 0,
                typed_digit: false,
                from_pointer: true,
            })
            .expect("engine alive");
        h.press(poltertype_types::SC_POINTER_BUTTON);
        h.release(poltertype_types::SC_POINTER_BUTTON);
        h.settle();
        let (ops, _) = h.stop();
        assert_eq!(
            ops,
            vec![
                EmitOp::Backspaces(6),
                EmitOp::Keys(HELLO.iter().copied().chain([SPACE]).collect()),
            ],
            "the queued click observation must not abort the accepted replacement"
        );
    }

    /// A click that did NOT land on the tooltip: the user clicked
    /// somewhere else and kept typing. The grace window must die on
    /// that first keypress, and a (hypothetical, late) accept must be
    /// declined — the caret is somewhere the engine can't vouch for.
    #[test]
    fn click_elsewhere_then_typing_kills_offer() {
        let h = suggestion_harness();
        type_word(&h, &HWLLO);
        h.tap(SPACE);
        let generation = ready_generation(&h);
        h.press(poltertype_types::SC_POINTER_BUTTON);
        h.release(poltertype_types::SC_POINTER_BUTTON);
        h.tap(0x1E); // `a` — typing resumes elsewhere
        let _ = h.wait_for(|e| matches!(e, SwitcherEvent::SuggestionsDismissed { .. }));
        h.cmd_tx
            .send(EngineCommand::AcceptSuggestion {
                generation,
                index: 0,
                typed_digit: false,
                from_pointer: true,
            })
            .expect("engine alive");
        h.settle();
        let (ops, _) = h.stop();
        assert!(
            ops.is_empty(),
            "an accept after the grace was voided must not touch the text"
        );
    }

    /// Regression for the two bugs the first live Hyprland run hit: the
    /// evdev listener stamps a modifier's own press with its flag, which
    /// read as a command and killed the accept chord; and pausing to
    /// *read* the tooltip past `idle_timeout_ms` voided the offer.
    #[test]
    fn accept_chord_survives_modifier_presses_and_idle_gap() {
        let h = Harness::start_full(
            400, // idle_timeout_ms — the pause below exceeds it
            MockEmitter::default(),
            false,
            Some(Arc::new(FixedSuggestions(vec!["hello"]))),
            Some(vec![Box::new(NoOpinionDetector)]),
        );
        type_word(&h, &HWLLO);
        h.tap(SPACE);
        let _generation = ready_generation(&h);
        std::thread::sleep(Duration::from_millis(700)); // reading the tooltip

        let m = |control: bool, shift: bool| poltertype_types::Modifiers {
            control,
            shift,
            ..poltertype_types::Modifiers::NONE
        };
        h.key_mods(0x1D, KeyDirection::Press, m(true, false)); // Ctrl↓
        h.key_mods(0x2A, KeyDirection::Press, m(true, true)); // Shift↓
        h.key_mods(0x02, KeyDirection::Press, m(true, true)); // 1↓
        h.key_mods(0x02, KeyDirection::Release, m(true, true));
        h.key_mods(0x2A, KeyDirection::Release, m(true, false));
        h.key_mods(0x1D, KeyDirection::Release, m(false, false));
        h.settle();
        let (ops, _) = h.stop();
        assert_eq!(
            ops,
            vec![
                // No `ReleaseModifiers`: this run lets Ctrl and Shift
                // back up while the correction is still absorbing, so by
                // the time it types nothing is held.
                EmitOp::Backspaces(7),
                EmitOp::Keys(HELLO.iter().copied().chain([SPACE]).collect()),
            ],
            "the accept chord must survive its own modifier presses and an idle-length pause"
        );
    }

    #[test]
    fn stale_generation_accept_is_ignored() {
        let h = suggestion_harness();
        type_word(&h, &HWLLO);
        h.tap(SPACE);
        let first = ready_generation(&h);
        // A second word completes → the first offer is dead.
        type_word(&h, &HWLLO);
        h.tap(SPACE);
        let second = ready_generation(&h);
        assert_ne!(first, second);
        h.cmd_tx
            .send(EngineCommand::AcceptSuggestion {
                generation: first,
                index: 0,
                typed_digit: false,
                from_pointer: false,
            })
            .expect("engine alive");
        h.settle();
        let (ops, _) = h.stop();
        assert!(ops.is_empty(), "a stale accept must not touch the text");
    }

    #[test]
    fn caret_jump_dismisses_offer() {
        let h = suggestion_harness();
        type_word(&h, &HWLLO);
        h.tap(SPACE);
        let generation = ready_generation(&h);
        h.tap(0x01); // Esc — caret context gone
        let ev = h.wait_for(|e| matches!(e, SwitcherEvent::SuggestionsDismissed { .. }));
        let SwitcherEvent::SuggestionsDismissed { generation: g } = ev else {
            unreachable!()
        };
        assert_eq!(g, generation);
        // A late accept after the dismissal must be a no-op.
        h.cmd_tx
            .send(EngineCommand::AcceptSuggestion {
                generation,
                index: 0,
                typed_digit: false,
                from_pointer: false,
            })
            .expect("engine alive");
        h.settle();
        let (ops, _) = h.stop();
        assert!(ops.is_empty());
    }

    #[test]
    fn low_confidence_alt_leads_entries_and_switches_on_accept() {
        let h = Harness::start_full(
            60_000,
            MockEmitter::default(),
            false,
            Some(Arc::new(FixedSuggestions(vec!["hello"]))),
            Some(vec![Box::new(TimidOther(
                LayoutId::from("en-US"),
                LayoutId::from("uk-UA"),
            ))]),
        );
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        let ev = h.wait_for(|e| matches!(e, SwitcherEvent::SuggestionsReady { .. }));
        let SwitcherEvent::SuggestionsReady {
            generation,
            entries,
            ..
        } = ev
        else {
            unreachable!()
        };
        assert_eq!(
            entries[0].switch_to,
            Some(LayoutId::from("uk-UA")),
            "below-threshold verdict must lead the entry list"
        );
        assert_eq!(entries[0].text, "привіт");
        h.cmd_tx
            .send(EngineCommand::AcceptSuggestion {
                generation,
                index: 0,
                typed_digit: false,
                from_pointer: false,
            })
            .expect("engine alive");
        h.settle();
        assert_eq!(
            *h.switcher.switches.lock(),
            vec![LayoutId::from("uk-UA")],
            "accepting the cross-layout entry must switch the layout"
        );
        let (ops, events) = h.stop();
        assert_eq!(
            ops,
            vec![
                EmitOp::Backspaces(7),
                EmitOp::Keys(GHBDSN.iter().copied().chain([SPACE]).collect()),
            ],
            "cross-layout accept replays the original scancodes"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SwitcherEvent::Corrected { .. })),
            "a cross-layout accept IS a layout correction"
        );
    }

    /// `[exceptions].word_whitelist` says "never auto-correct this
    /// word"; it once only silenced the suggestion tooltip while the
    /// correction went ahead regardless. The detector here switches
    /// everything it is shown, so anything reaching it corrects.
    #[test]
    fn whitelisted_word_is_not_auto_corrected() {
        let h = Harness::start_configured(
            60_000,
            MockEmitter::default(),
            false,
            None,
            None,
            None,
            |s| s.exceptions.word_whitelist = vec!["GHBDSN".into()],
            None,
        );
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.settle();
        let (ops, events) = h.stop();
        assert!(
            ops.is_empty(),
            "a whitelisted word must not be touched, got {ops:?}"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SwitcherEvent::KeptCurrent { reason } if reason.contains("whitelist"))),
            "the decision trail must name the whitelist as the reason"
        );
    }

    /// The same hotkey, pressed the way a person presses one: after a
    /// pause long enough to notice the layout was wrong.
    ///
    /// Idle hygiene abandons the in-progress buffer, and used to drop
    /// the stash with it — including on the chord's own Ctrl press,
    /// which is itself a key event and arrives after the pause. Nobody
    /// reaches for a chord inside `idle_timeout_ms` (two seconds by
    /// default), so the one path that exists for "the automatic pass
    /// did not fire" was dead on every press a person could make.
    #[test]
    fn manual_hotkey_survives_an_idle_pause() {
        let h = Harness::start(50); // 50 ms idle timeout
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.wait_for(|e| matches!(e, SwitcherEvent::Corrected { .. }));
        h.settle();

        std::thread::sleep(Duration::from_millis(120));
        h.tap(0x1D); // the chord's own Ctrl, long after the timeout
        h.settle();

        h.cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        h.wait_for(|e| matches!(e, SwitcherEvent::AddToDictionary { .. }));
        h.settle();
        assert_eq!(
            *h.switcher.switches.lock(),
            vec![LayoutId::from("uk-UA"), LayoutId::from("en-US")],
            "the hotkey must still reach the last word after a pause"
        );
    }

    /// A chord whose key also closes words must keep working.
    ///
    /// The default pause chord is `Ctrl+Shift+Space`, and Space is what
    /// closes most words — so its release lands inside the correction
    /// that word triggered, where the window reads key events straight
    /// off the channel. The latch that makes a chord fire once per
    /// physical press then never cleared, and pause was dead for the
    /// rest of the session. The force-switch had the milder form of it,
    /// answering every *other* press; that is what this was found by,
    /// live on KDE Plasma Wayland while checking issue #37.
    #[test]
    fn a_chord_still_fires_after_its_key_closed_a_corrected_word() {
        let mods = poltertype_types::Modifiers {
            control: true,
            shift: true,
            ..poltertype_types::Modifiers::NONE
        };
        let h = Harness::start(60_000);
        h.cmd_tx
            .send(EngineCommand::SetKeystreamHotkeys(KeystreamHotkeys {
                grabbed: [None, None],
                pause: Some(Binding::Key(Chord {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    meta: false,
                    scancode: SPACE,
                })),
                switch_last: None,
            }))
            .expect("engine alive");

        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.wait_for(|e| matches!(e, SwitcherEvent::Corrected { .. }));
        h.settle();

        h.key_mods(SPACE, KeyDirection::Press, mods);
        h.key_mods(SPACE, KeyDirection::Release, mods);
        h.wait_for(|e| matches!(e, SwitcherEvent::PausedChanged(true)));
    }

    /// The whole point of issue #32: the gesture Punto and Caramba
    /// users already have in their hands has to reach the same
    /// force-switch the command does — off the key stream, with no key
    /// code and no OS-level grab anywhere in the path.
    #[test]
    fn a_double_shift_tap_force_switches_the_last_word() {
        const L_SHIFT: u32 = 0x2A;
        let h = Harness::start(60_000);
        h.cmd_tx
            .send(EngineCommand::SetKeystreamHotkeys(KeystreamHotkeys {
                grabbed: [None, None],
                pause: None,
                switch_last: Some(Binding::Mods(ModChord {
                    mods: ModSet {
                        shift: true,
                        ..ModSet::NONE
                    },
                    double_tap: true,
                })),
            }))
            .expect("engine alive");
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.wait_for(|e| matches!(e, SwitcherEvent::Corrected { .. }));
        h.settle();

        // Two taps, back to back: the pair has to land inside the
        // double-tap window, which is why nothing settles in between.
        h.tap(L_SHIFT);
        h.tap(L_SHIFT);
        h.wait_for(|e| matches!(e, SwitcherEvent::AddToDictionary { .. }));
        h.settle();

        assert_eq!(
            *h.switcher.switches.lock(),
            vec![LayoutId::from("uk-UA"), LayoutId::from("en-US")],
            "the second tap must undo the correction the first one did not"
        );
    }

    /// The same gesture with a letter typed between the taps is
    /// somebody typing, not somebody asking for anything.
    #[test]
    fn a_shift_hold_around_a_letter_does_not_force_switch() {
        const L_SHIFT: u32 = 0x2A;
        let h = Harness::start(60_000);
        h.cmd_tx
            .send(EngineCommand::SetKeystreamHotkeys(KeystreamHotkeys {
                grabbed: [None, None],
                pause: None,
                switch_last: Some(Binding::Mods(ModChord {
                    mods: ModSet {
                        shift: true,
                        ..ModSet::NONE
                    },
                    double_tap: true,
                })),
            }))
            .expect("engine alive");
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.wait_for(|e| matches!(e, SwitcherEvent::Corrected { .. }));
        h.settle();
        let after_correction = h.switcher.switches.lock().clone();

        for _ in 0..2 {
            h.press(L_SHIFT);
            h.tap(0x1E); // a capital A
            h.release(L_SHIFT);
        }
        h.settle();

        assert_eq!(
            *h.switcher.switches.lock(),
            after_correction,
            "typing capitals must not reach the force-switch"
        );
    }

    /// The manual hotkey after one of our own corrections puts the word
    /// back (re-applying the same correction deleted it and retyped it
    /// identically) and takes the rescued word into the user's
    /// dictionary — the only route the auto-correction path has in.
    #[test]
    fn manual_hotkey_undoes_a_correction_and_learns_the_word() {
        let h = Harness::start(60_000);
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.wait_for(|e| matches!(e, SwitcherEvent::Corrected { .. }));
        h.settle();
        assert_eq!(
            *h.switcher.switches.lock(),
            vec![LayoutId::from("uk-UA")],
            "precondition: the engine corrected into uk-UA"
        );

        h.cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        let ev = h.wait_for(|e| matches!(e, SwitcherEvent::AddToDictionary { .. }));
        let SwitcherEvent::AddToDictionary {
            layout,
            word,
            origin,
        } = ev
        else {
            unreachable!()
        };
        assert_eq!(
            layout,
            LayoutId::from("en-US"),
            "learn it where it was typed"
        );
        assert_eq!(word, "ghbdsn");
        assert_eq!(origin, DictionaryAddOrigin::UndoneCorrection);
        h.settle();
        assert_eq!(
            *h.switcher.switches.lock(),
            vec![LayoutId::from("uk-UA"), LayoutId::from("en-US")],
            "the undo has to switch the layout back too"
        );
    }

    /// Undoing a correction the engine got *right* must not teach the
    /// dictionary its wrong-layout twin. `ghbdsn` reached the real
    /// en-US overlay exactly this way — an undo of a correction backed
    /// by uk-UA `привіт` — and then rewrote every correctly typed
    /// `привіт` back into itself, for good.
    #[test]
    fn manual_hotkey_undo_does_not_learn_a_wrong_layout_twin() {
        let h = Harness::start_full(
            60_000,
            MockEmitter::default(),
            false,
            Some(Arc::new(KnownWords(&[("uk-UA", "привіт")]))),
            None,
        );
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.wait_for(|e| matches!(e, SwitcherEvent::Corrected { .. }));
        h.settle();

        h.cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        h.settle();
        let switches = h.switcher.switches.lock().clone();
        let (_, events) = h.stop();
        assert_eq!(
            switches,
            vec![LayoutId::from("uk-UA"), LayoutId::from("en-US")],
            "the undo itself must still happen — only the learning is withheld"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, SwitcherEvent::AddToDictionary { .. })),
            "the correction rested on a real uk-UA word, so its en-US twin is not one"
        );
    }

    /// Counter-regression: with no dictionary evidence behind the
    /// correction, the undo still means "this is a word" and still
    /// teaches it. Without this the guard would silently retire the
    /// auto-correction path's only escape hatch.
    #[test]
    fn manual_hotkey_undo_still_learns_when_the_switch_was_a_guess() {
        let h = Harness::start_full(
            60_000,
            MockEmitter::default(),
            false,
            Some(Arc::new(KnownWords(&[]))),
            None,
        );
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.wait_for(|e| matches!(e, SwitcherEvent::Corrected { .. }));
        h.settle();

        h.cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        let ev = h.wait_for(|e| matches!(e, SwitcherEvent::AddToDictionary { .. }));
        let SwitcherEvent::AddToDictionary { layout, word, .. } = ev else {
            unreachable!()
        };
        assert_eq!(layout, LayoutId::from("en-US"));
        assert_eq!(word, "ghbdsn");
    }

    /// Pause stops the engine deciding, not the engine watching.
    ///
    /// A user who turns auto-switch off because they dislike its false
    /// positives is exactly the user who reaches for the manual hotkey
    /// — and it did nothing at all, because `handle_key` returned
    /// before the buffer could track the word and the stash the hotkey
    /// reads is written at a word boundary (issue #36).
    #[test]
    fn the_manual_hotkey_still_works_while_paused() {
        let h = Harness::start(60_000);
        h.cmd_tx
            .send(EngineCommand::TogglePause)
            .expect("engine alive");
        h.wait_for(|e| matches!(e, SwitcherEvent::PausedChanged(true)));

        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.settle();
        assert!(
            h.switcher.switches.lock().is_empty(),
            "paused must still mean no automatic correction"
        );

        h.cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        h.wait_for(|e| matches!(e, SwitcherEvent::Corrected { .. }));
        h.settle();
        assert_eq!(
            h.switcher.switches.lock().len(),
            1,
            "the hotkey must switch the word the pause left alone"
        );
    }

    /// Turning auto-switch off is a decision, not a mood: the app used
    /// to forget it on quit, so the next launch went back to correcting
    /// words for somebody who had switched that off (issue #46).
    #[test]
    fn auto_switch_left_off_in_the_config_comes_back_off() {
        let h = Harness::start_configured(
            60_000,
            MockEmitter::default(),
            false,
            None,
            None,
            None,
            |s| s.general.paused = true,
            None,
        );
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.settle();
        assert!(
            h.switcher.switches.lock().is_empty(),
            "a config that says auto-switch was left off must start it off"
        );

        h.cmd_tx
            .send(EngineCommand::SetPaused(false))
            .expect("engine alive");
        h.wait_for(|e| matches!(e, SwitcherEvent::PausedChanged(false)));
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.wait_for(|e| matches!(e, SwitcherEvent::Corrected { .. }));
    }

    /// An app that came back paused has to be resumable by the chord —
    /// the pause hotkey is matched before the paused early-return, and
    /// a build that started paused would otherwise have no way back
    /// except the tray.
    #[test]
    fn the_pause_chord_resumes_an_app_that_started_paused() {
        let h = Harness::start_configured(
            60_000,
            MockEmitter::default(),
            false,
            None,
            None,
            None,
            |s| s.general.paused = true,
            None,
        );
        h.cmd_tx
            .send(EngineCommand::SetKeystreamHotkeys(KeystreamHotkeys {
                grabbed: [None, None],
                pause: Some(Binding::Key(Chord {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    meta: false,
                    scancode: 0x43,
                })),
                switch_last: None,
            }))
            .expect("engine alive");

        let none = poltertype_types::Modifiers::NONE;
        let ctrl = poltertype_types::Modifiers {
            control: true,
            ..none
        };
        let both = poltertype_types::Modifiers {
            control: true,
            shift: true,
            ..none
        };
        h.key_mods(0x1D, KeyDirection::Press, ctrl);
        h.key_mods(0x2A, KeyDirection::Press, both);
        h.key_mods(0x43, KeyDirection::Press, both);
        h.key_mods(0x43, KeyDirection::Release, both);
        h.key_mods(0x2A, KeyDirection::Release, ctrl);
        h.key_mods(0x1D, KeyDirection::Release, none);
        h.wait_for(|e| matches!(e, SwitcherEvent::PausedChanged(false)));
    }

    /// The config file names a state, and the watcher re-applies it
    /// after *any* edit to the file. Read as a toggle, changing the
    /// sound theme would resume a paused app.
    #[test]
    fn being_told_the_pause_state_it_is_already_in_changes_nothing() {
        let h = Harness::start_configured(
            60_000,
            MockEmitter::default(),
            false,
            None,
            None,
            None,
            |s| s.general.paused = true,
            None,
        );
        h.cmd_tx
            .send(EngineCommand::SetPaused(true))
            .expect("engine alive");
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.settle();
        assert!(
            h.switcher.switches.lock().is_empty(),
            "still paused, so still no correction"
        );
        let (_, events) = h.stop();
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, SwitcherEvent::PausedChanged(_))),
            "nothing changed, so the tray must not be told anything did"
        );
    }

    /// The gesture people actually arrive with: type the word, see the
    /// wrong layout, press the key — with no space anywhere in it.
    ///
    /// Until 0.20.0 the stash was written only when a word was closed,
    /// so this logged "no last word stashed" and did nothing at all.
    /// Measured that way on KDE Plasma Wayland against 0.19.0, and
    /// reported as the force-switch hotkey simply not working (#34,
    /// and the first request in #32).
    #[test]
    fn the_hotkey_switches_a_word_that_has_no_separator_after_it_yet() {
        let h = Harness::start(60_000);
        type_word(&h, &GHBDSN); // no boundary key at all
        h.settle();

        h.cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        h.wait_for(|e| matches!(e, SwitcherEvent::Corrected { .. }));
        h.settle();

        assert_eq!(
            *h.switcher.switches.lock(),
            vec![LayoutId::from("uk-UA")],
            "the word under the fingers must switch like a closed one"
        );
    }

    /// …and it retypes the word alone. A closed word is backspaced over
    /// together with its separator; an unfinished one has no separator,
    /// and eating the character after the caret would take something
    /// the user never typed.
    #[test]
    fn switching_an_unfinished_word_leaves_the_caret_where_it_was() {
        let h = Harness::start(60_000);
        type_word(&h, &GHBDSN);
        h.settle();

        h.cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        h.wait_for(|e| matches!(e, SwitcherEvent::Corrected { .. }));
        h.settle();

        let first_erase = h.emitter.ops().iter().find_map(|o| match o {
            EmitOp::Backspaces(n) => Some(*n),
            _ => None,
        });
        assert_eq!(
            first_erase,
            Some(GHBDSN.len()),
            "exactly the word, and nothing past the caret"
        );
    }

    /// A held chord must not switch the same word over and over —
    /// `wow ` once became `wow wow wow…` until the app was killed.
    /// Sent as a burst, which is what auto-repeat and the fire our own
    /// Backspaces provoke actually look like: everything inside
    /// `FORCE_SWITCH_REARM` of the first is one press.
    #[test]
    fn a_repeated_hotkey_switches_an_unfinished_word_only_once() {
        let h = Harness::start(60_000);
        type_word(&h, &GHBDSN);
        h.settle();

        for _ in 0..3 {
            h.cmd_tx
                .send(EngineCommand::SwitchLastForcefully)
                .expect("engine alive");
        }
        h.settle();

        assert_eq!(
            h.switcher.switches.lock().len(),
            1,
            "auto-repeat must not keep re-switching the same word"
        );
    }

    /// …but a *deliberate* second press must be honoured. The stash
    /// used to be consumed to reach the switch, so the hotkey worked
    /// exactly once per word and a press made in error could not be
    /// taken back (issue #37).
    #[test]
    fn pressing_the_hotkey_again_switches_the_word_back() {
        let h = Harness::start(60_000);
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.wait_for(|e| matches!(e, SwitcherEvent::Corrected { .. }));
        h.settle();
        let after_auto = h.switcher.switches.lock().len();

        for _ in 0..2 {
            h.cmd_tx
                .send(EngineCommand::SwitchLastForcefully)
                .expect("engine alive");
            h.wait_for(|e| matches!(e, SwitcherEvent::Corrected { .. }));
            h.settle();
        }

        assert_eq!(
            h.switcher.switches.lock().len(),
            after_auto + 2,
            "the second press must switch the word back, not do nothing"
        );
        let layouts = h.switcher.switches.lock().clone();
        assert_eq!(
            layouts.last(),
            layouts.get(after_auto - 1),
            "two presses land the word where the engine had put it"
        );
    }

    /// Undoing an engine correction teaches the word; taking back a
    /// press of the user's own must not. A third press has the shape of
    /// an undo — `corrected_to` is set — but it was this hotkey that
    /// set it, and taking back your own press says nothing about the
    /// word. Without `user_placed` a user tapping through the
    /// renderings would silently disable correction for it forever.
    #[test]
    fn taking_back_your_own_press_does_not_teach_the_dictionary() {
        let h = Harness::start(60_000);
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.wait_for(|e| matches!(e, SwitcherEvent::Corrected { .. }));
        h.settle();

        // One: undo the engine, which teaches — the escape hatch pinned
        // by `manual_hotkey_undo_still_learns_when_the_switch_was_a_guess`.
        h.cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        h.wait_for(|e| matches!(e, SwitcherEvent::AddToDictionary { .. }));
        h.settle();
        // Two: rotate back to where the engine had put it.
        h.cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        h.wait_for(|e| matches!(e, SwitcherEvent::Corrected { .. }));
        h.settle();
        while h.out_rx.try_recv().is_ok() {}

        // Three: the one that used to look like an undo.
        h.cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        h.wait_for(|e| matches!(e, SwitcherEvent::Corrected { .. }));
        h.settle();

        let taught = std::iter::from_fn(|| h.out_rx.try_recv().ok())
            .filter(|e| matches!(e, SwitcherEvent::AddToDictionary { .. }))
            .count();
        assert_eq!(
            taught, 0,
            "only the press that undid the engine's own correction teaches"
        );
    }

    /// The hotkey pressed a moment late, once the next word has
    /// started, must act on *that* word.
    ///
    /// A correction backspaces from the caret. The stash still names
    /// the previous word, so using it here sent that word's backspace
    /// count into text several characters to its right and left the
    /// line in pieces — with no way for the user to guess why. Present
    /// since the hotkey existed; a plausible half of the "sometimes
    /// random symbols" in #33.
    #[test]
    fn the_hotkey_acts_on_the_word_the_caret_is_actually_in() {
        let h = Harness::start(60_000);
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.wait_for(|e| matches!(e, SwitcherEvent::Corrected { .. }));
        h.settle();
        // Three letters of a second word, deliberately unfinished.
        let second = [0x11u32, 0x18, 0x13];
        for sc in second {
            h.tap(sc);
        }
        h.settle();

        h.cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        h.settle();

        let erases: Vec<usize> = h
            .emitter
            .ops()
            .iter()
            .filter_map(|o| match o {
                EmitOp::Backspaces(n) => Some(*n),
                _ => None,
            })
            .collect();
        assert_eq!(
            erases.last(),
            Some(&second.len()),
            "the erase must match the word under the caret, not the one before it"
        );
    }

    /// …and once the caret is past more separators than the stash
    /// recorded, there is nothing safe to switch at all.
    #[test]
    fn the_hotkey_gives_up_when_the_caret_has_moved_past_the_stash() {
        let h = Harness::start(60_000);
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.wait_for(|e| matches!(e, SwitcherEvent::Corrected { .. }));
        h.settle();
        let before = h.emitter.ops().len();
        h.tap(SPACE); // a second separator the stash knows nothing about
        h.settle();

        h.cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        h.settle();

        assert_eq!(
            h.emitter.ops().len(),
            before,
            "nothing may be typed when the caret cannot be accounted for"
        );
    }

    /// A buffer that lost the caret is not a word to switch: the
    /// correction would land wherever the caret actually is.
    #[test]
    fn a_poisoned_buffer_is_left_alone_by_the_hotkey() {
        let h = Harness::start(50);
        type_word(&h, &GHBDSN);
        // The idle sweep abandons the word in flight, which poisons.
        std::thread::sleep(Duration::from_millis(160));
        h.tap(0x1D);
        h.settle();

        h.cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        h.settle();

        assert!(
            h.switcher.switches.lock().is_empty(),
            "a caret we cannot vouch for must not be typed over"
        );
    }

    /// The same hotkey on a word the engine *left alone* keeps its
    /// original meaning — apply the switch we declined — and teaches
    /// nothing: the user is telling us to correct that word, which is
    /// the opposite of "this word is fine as typed".
    #[test]
    fn manual_hotkey_on_a_kept_word_switches_without_learning() {
        let h = Harness::start_full(
            60_000,
            MockEmitter::default(),
            false,
            None,
            Some(vec![Box::new(NoOpinionDetector)]),
        );
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.settle();
        h.cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        // No wait for `Corrected`: with no correction to reverse the
        // hotkey falls back to "some other layout", which may not be
        // active in the mock OS — the assertion below is the point.
        h.settle();
        let (_, events) = h.stop();
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, SwitcherEvent::AddToDictionary { .. })),
            "forcing a switch must not add the pre-switch word to the dictionary"
        );
    }

    /// Issue #40: the hotkey must keep working on the words typed
    /// *after* one it has already switched.
    ///
    /// Switching a word still being typed used to `abandon` the buffer,
    /// which says "the caret is lost" — and a lost caret is refused,
    /// so the gesture went dead for every following word until a
    /// separator cleared the taint. The user sees a hotkey that stops
    /// answering, or answers with the word before.
    #[test]
    fn the_hotkey_still_works_on_the_word_typed_after_a_switched_one() {
        let h = Harness::start(60_000);
        type_word(&h, &GHBDSN);
        h.settle();
        // Two presses: out to the other layout and back, the shape in
        // the report.
        for _ in 0..2 {
            h.cmd_tx
                .send(EngineCommand::SwitchLastForcefully)
                .expect("engine alive");
            h.settle();
        }
        // Rub the word out and type a shorter one in its place.
        for _ in 0..GHBDSN.len() {
            h.tap(BACKSPACE);
        }
        let second = [0x32u32, 0x18, 0x18];
        for sc in second {
            h.tap(sc);
        }
        h.settle();

        h.cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        h.settle();

        let erases: Vec<usize> = h
            .emitter
            .ops()
            .iter()
            .filter_map(|o| match o {
                EmitOp::Backspaces(n) => Some(*n),
                _ => None,
            })
            .collect();
        assert_eq!(
            erases.last(),
            Some(&second.len()),
            "the erase must match the word now under the caret"
        );
    }

    /// Issue #44: clearing the line with `Ctrl+Backspace` left the
    /// force-switch dead for every word typed afterwards.
    ///
    /// A shortcut taints the word in flight, and the taint outlives it
    /// — it is cleared only at the next boundary, so the word typed
    /// *next*, watched from its first key, was refused too. A leftward
    /// word-delete is the one shortcut that cannot leave an unrecorded
    /// remainder behind: it erases the very text the taint exists to
    /// protect.
    #[test]
    fn the_hotkey_survives_a_line_cleared_with_ctrl_backspace() {
        let h = Harness::start(60_000);
        type_word(&h, &GHBDSN);
        h.settle();
        // "Manually switching several times" — out and back.
        for _ in 0..2 {
            h.cmd_tx
                .send(EngineCommand::SwitchLastForcefully)
                .expect("engine alive");
            h.settle();
        }
        ctrl_backspace(&h);
        let second = [0x32u32, 0x18, 0x18];
        for sc in second {
            h.tap(sc);
        }
        h.settle();

        h.cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        h.settle();

        assert_eq!(
            erase_counts(&h).last(),
            Some(&second.len()),
            "the word typed after a word-delete is watched from its first key: {:?}",
            h.emitter.ops()
        );
    }

    /// The other way the report's line gets cleared: select the lot,
    /// then rub it out with a plain Backspace. The taint is set by
    /// `Ctrl+A` and survives the deletion that makes it meaningless.
    #[test]
    fn the_hotkey_survives_a_line_cleared_with_select_all_and_backspace() {
        let h = Harness::start(60_000);
        type_word(&h, &GHBDSN);
        h.settle();
        let none = poltertype_types::Modifiers::NONE;
        let ctrl = poltertype_types::Modifiers {
            control: true,
            ..none
        };
        h.key_mods(0x1D, KeyDirection::Press, ctrl);
        h.key_mods(0x1E, KeyDirection::Press, ctrl); // A
        h.key_mods(0x1E, KeyDirection::Release, ctrl);
        h.key_mods(0x1D, KeyDirection::Release, none);
        h.tap(BACKSPACE);
        let second = [0x32u32, 0x18, 0x18];
        for sc in second {
            h.tap(sc);
        }
        h.settle();

        h.cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        h.settle();

        assert_eq!(
            erase_counts(&h).last(),
            Some(&second.len()),
            "backspacing past everything we track leaves nothing to splice into: {:?}",
            h.emitter.ops()
        );
    }

    /// The same root, on the automatic path: auto-switch went quiet for
    /// the first word typed after a line was cleared with the shortcut.
    #[test]
    fn auto_switch_still_fires_on_the_word_after_ctrl_backspace() {
        let h = Harness::start(60_000);
        type_word(&h, &GHBDSN);
        h.settle();
        ctrl_backspace(&h);
        type_word(&h, &GHBDSN);
        h.tap(SPACE);

        h.wait_for(|e| matches!(e, SwitcherEvent::Corrected { .. }));
    }

    /// The fence around the three above. A shortcut that deletes
    /// nothing has to go on tainting: the word it interrupted is still
    /// on screen, immediately left of the caret, so a correction of the
    /// next one would count characters it cannot see and splice into
    /// its tail.
    #[test]
    fn a_shortcut_that_deletes_nothing_still_stops_the_hotkey() {
        let h = Harness::start(60_000);
        type_word(&h, &GHBDSN);
        h.settle();
        let none = poltertype_types::Modifiers::NONE;
        let ctrl = poltertype_types::Modifiers {
            control: true,
            ..none
        };
        h.key_mods(0x1D, KeyDirection::Press, ctrl);
        h.key_mods(0x2E, KeyDirection::Press, ctrl); // C — copies, edits nothing
        h.key_mods(0x2E, KeyDirection::Release, ctrl);
        h.key_mods(0x1D, KeyDirection::Release, none);
        h.settle();
        let before = erase_counts(&h).len();
        for sc in [0x32u32, 0x18, 0x18] {
            h.tap(sc);
        }
        h.settle();

        h.cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        h.settle();

        assert_eq!(
            erase_counts(&h).len(),
            before,
            "the abandoned word is still on screen: {:?}",
            h.emitter.ops()
        );
    }

    /// One `Ctrl+Backspace`, modifier edges included — the shape the
    /// listener reports it in.
    fn ctrl_backspace(h: &Harness) {
        let none = poltertype_types::Modifiers::NONE;
        let ctrl = poltertype_types::Modifiers {
            control: true,
            ..none
        };
        h.key_mods(0x1D, KeyDirection::Press, ctrl);
        h.key_mods(BACKSPACE, KeyDirection::Press, ctrl);
        h.key_mods(BACKSPACE, KeyDirection::Release, ctrl);
        h.key_mods(0x1D, KeyDirection::Release, none);
        h.settle();
    }

    /// Wait until the engine has taken every key sent so far.
    ///
    /// What a chord test actually needs before pressing the chord's own
    /// key is that the *modifiers have been observed*. A fixed sleep is
    /// a guess about someone else's CPU, and a guess that is wrong
    /// reads as a product failure — the correction finds a bare
    /// modifier press still queued and aborts as though the user had
    /// typed a shortcut.
    fn wait_until_taken(h: &Harness) {
        for _ in 0..500 {
            if h.key_tx.is_empty() {
                // Taken off the channel is not yet handled; one more
                // beat covers the run loop's own dispatch.
                std::thread::sleep(Duration::from_millis(50));
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("engine never drained the key channel");
    }

    /// Announce the default switch-last chord as an OS-level grab —
    /// what every backend but Wayland does with it.
    fn grab_default_switch_chord(h: &Harness, sc: u32) {
        h.cmd_tx
            .send(EngineCommand::SetKeystreamHotkeys(KeystreamHotkeys {
                pause: None,
                switch_last: None,
                grabbed: [
                    None,
                    Some(Chord {
                        ctrl: true,
                        shift: true,
                        alt: false,
                        meta: false,
                        scancode: sc,
                    }),
                ],
            }))
            .expect("engine alive");
        h.settle();
    }

    /// One press of that chord, then the grab's command, then the
    /// modifiers coming back up — the order the machine reports it in.
    /// The grabbed key's own release never arrives (see
    /// `a_grabbed_chord_held_down_is_waited_out_rather_than_typed_over`).
    fn press_grabbed_switch_chord(h: &Harness, sc: u32) {
        let none = poltertype_types::Modifiers::NONE;
        let ctrl = poltertype_types::Modifiers {
            control: true,
            ..none
        };
        let both = poltertype_types::Modifiers {
            shift: true,
            ..ctrl
        };
        h.key_mods(0x1D, KeyDirection::Press, ctrl);
        h.key_mods(0x2A, KeyDirection::Press, both);
        // The run loop has to take the modifiers before the chord
        // fires — a person's fingers arrive in that order, and a bare
        // modifier press still queued reads as a shortcut the
        // correction cannot reconstruct. See
        // `a_grabbed_chord_held_down_is_waited_out_rather_than_typed_over`.
        wait_until_taken(h);
        h.key_mods(sc, KeyDirection::Press, both);
        h.cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        h.key_mods(0x2A, KeyDirection::Release, ctrl);
        h.key_mods(0x1D, KeyDirection::Release, none);
    }

    /// The default switch-last chord is `Ctrl+Shift+Backspace`, and
    /// outside Wayland an OS grab owns it — which hides the chord from
    /// our matcher but not the key from our listener. Its own press
    /// therefore arrives here in the exact shape of the word-delete
    /// below it in `handle_key`, and reaches us *before* the desktop
    /// delivers the hotkey. Read that way it would drop the stash the
    /// command right behind it is about to switch, which with
    /// auto-switch paused is the only conversion there is (issue #51).
    #[test]
    fn the_default_grabbed_chord_is_not_read_as_a_word_delete() {
        let h = Harness::start(60_000);
        h.cmd_tx
            .send(EngineCommand::TogglePause)
            .expect("engine alive");
        h.wait_for(|e| matches!(e, SwitcherEvent::PausedChanged(true)));
        grab_default_switch_chord(&h, BACKSPACE);

        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.settle();
        press_grabbed_switch_chord(&h, BACKSPACE);

        h.wait_for(|e| matches!(e, SwitcherEvent::Corrected { .. }));
        h.settle();
        assert_eq!(
            erase_counts(&h).last(),
            Some(&(GHBDSN.len() + 1)),
            "the word and its boundary, not a word-delete: {:?}",
            h.emitter.ops()
        );
    }

    /// The same press with no separator typed yet: there the
    /// word-delete reading would empty the buffer the hotkey falls
    /// back to, leaving nothing to switch by either route.
    #[test]
    fn the_default_grabbed_chord_switches_a_word_still_being_typed() {
        let h = Harness::start(60_000);
        grab_default_switch_chord(&h, BACKSPACE);
        type_word(&h, &GHBDSN);
        h.settle();
        press_grabbed_switch_chord(&h, BACKSPACE);

        h.wait_for(|e| matches!(e, SwitcherEvent::Corrected { .. }));
        h.settle();
        assert_eq!(
            erase_counts(&h).last(),
            Some(&GHBDSN.len()),
            "an unfinished word has no boundary to put back: {:?}",
            h.emitter.ops()
        );
    }

    /// `Shift+3`, with its modifier edges — `№` under uk-UA and `#`
    /// under en-US, and a separator under both.
    fn type_shifted(h: &Harness, sc: u32) {
        let shifted = poltertype_types::Modifiers {
            shift: true,
            ..poltertype_types::Modifiers::NONE
        };
        h.key_mods(0x2A, KeyDirection::Press, shifted);
        h.key_mods(sc, KeyDirection::Press, shifted);
        h.key_mods(sc, KeyDirection::Release, shifted);
        h.key_mods(
            0x2A,
            KeyDirection::Release,
            poltertype_types::Modifiers::NONE,
        );
    }

    /// Issue #52: `№` is `Shift+3`, which is not a letter in any
    /// layout — so it never joins a word, never reaches the stash, and
    /// the hotkey found nothing to switch. The key is known all the
    /// same, and one character is exactly what the report asked for.
    #[test]
    fn the_hotkey_switches_a_separator_when_there_is_no_word() {
        let h = Harness::start(60_000);
        type_shifted(&h, 0x04);
        h.settle();
        assert!(
            h.emitter.ops().is_empty(),
            "a separator on its own decides nothing automatically: {:?}",
            h.emitter.ops()
        );

        h.cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        h.wait_for(|e| matches!(e, SwitcherEvent::Corrected { .. }));
        h.settle();
        assert_eq!(
            h.emitter.ops(),
            vec![EmitOp::Backspaces(1), EmitOp::Keys(vec![0x04])],
            "one character erased and the same key retyped under the other layout"
        );
    }

    /// The same key after a word that has already been converted: the
    /// stash is gone, the separator is not.
    #[test]
    fn the_hotkey_reaches_the_separator_typed_after_a_word() {
        let h = Harness::start(60_000);
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.wait_for(|e| matches!(e, SwitcherEvent::Corrected { .. }));
        h.settle();
        type_shifted(&h, 0x04);
        h.settle();
        let before = h.emitter.ops().len();

        h.cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        h.settle();
        assert_eq!(
            h.emitter.ops()[before..],
            [EmitOp::Backspaces(1), EmitOp::Keys(vec![0x04])],
            "the caret is after the separator, not after the word: {:?}",
            h.emitter.ops()
        );
    }

    /// The stash outlives an idle abandon on purpose — that is what
    /// keeps the hotkey working after a pause (issue #44) — but the
    /// buffer drops the word with it. A separator typed in between
    /// therefore leaves nobody counting the characters between the
    /// caret and that word, and switching it anyway spliced the
    /// replacement one character too far right: `привет` + a pause +
    /// `№` came back as `пghbdtn `. Measured on Cinnamon X11.
    #[test]
    fn a_separator_typed_after_an_idle_pause_protects_the_word_behind_it() {
        let h = Harness::start(200);
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.wait_for(|e| matches!(e, SwitcherEvent::Corrected { .. }));
        h.settle();
        let before = h.emitter.ops().len();
        // Past the idle timeout: the buffer abandons the word, the
        // stash outlives it.
        std::thread::sleep(Duration::from_millis(400));
        type_shifted(&h, 0x04);
        h.settle();

        h.cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        h.settle();
        assert_eq!(
            h.emitter.ops()[before..],
            [EmitOp::Backspaces(1), EmitOp::Keys(vec![0x04])],
            "the separator is switchable; the word behind it is not measurable: {:?}",
            h.emitter.ops()
        );
    }

    /// The fence. A separator that reads the same under both layouts
    /// has nothing to switch, and retyping it would move the caret for
    /// no reason — a space most of all, which is every layout's space.
    #[test]
    fn the_hotkey_leaves_a_separator_that_means_the_same_thing_alone() {
        let h = Harness::start(60_000);
        h.tap(SPACE);
        h.settle();

        h.cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        h.settle();
        assert!(
            h.emitter.ops().is_empty(),
            "nothing to switch about a space: {:?}",
            h.emitter.ops()
        );
    }

    /// And Enter is never replayed: pressing it again submits the line
    /// the user is looking at rather than typing a character.
    #[test]
    fn the_hotkey_never_replays_a_submission_key() {
        let h = Harness::start(60_000);
        h.tap(0x1C);
        h.settle();

        h.cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        h.settle();
        assert!(
            h.emitter.ops().is_empty(),
            "Enter must not be retyped: {:?}",
            h.emitter.ops()
        );
    }

    /// Issue #47: the force-switch chimed whatever the setting said.
    ///
    /// Every other path builds its `Correction` with
    /// `general.sound_on_correct`; this one carried a literal `true`,
    /// so "Play a soft chime on correction" turned off the automatic
    /// chime and left the manual one ringing.
    #[test]
    fn the_force_switch_obeys_the_chime_setting() {
        let quiet = Harness::start_configured(
            60_000,
            MockEmitter::default(),
            false,
            None,
            None,
            None,
            |s| s.general.sound_on_correct = false,
            None,
        );
        type_word(&quiet, &GHBDSN);
        quiet.settle();
        quiet
            .cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        quiet.settle();
        assert!(
            !erase_counts(&quiet).is_empty(),
            "the switch itself must still happen"
        );
        assert!(
            quiet.sounds().is_empty(),
            "chime off means no chime: {:?}",
            quiet.sounds()
        );

        let loud = Harness::start(60_000);
        type_word(&loud, &GHBDSN);
        loud.settle();
        loud.cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        loud.settle();
        assert!(
            matches!(
                loud.sounds().as_slice(),
                [crate::audio::SoundEvent::Correct, ..]
            ),
            "chime on means a chime: {:?}",
            loud.sounds()
        );
    }

    /// Two taps of the chord, one after the other, must be two
    /// corrections.
    #[test]
    fn two_taps_of_the_chord_are_two_corrections() {
        let h = Harness::start(60_000);
        h.cmd_tx
            .send(EngineCommand::SetKeystreamHotkeys(KeystreamHotkeys {
                grabbed: [None, None],
                pause: None,
                switch_last: Some(Binding::Key(Chord {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    meta: false,
                    scancode: 0x43,
                })),
            }))
            .expect("engine alive");
        let none = poltertype_types::Modifiers::NONE;
        let ctrl = poltertype_types::Modifiers {
            control: true,
            ..none
        };
        let both = poltertype_types::Modifiers {
            control: true,
            shift: true,
            ..none
        };
        let tap_chord = |h: &Harness| {
            h.key_mods(0x1D, KeyDirection::Press, ctrl);
            h.key_mods(0x2A, KeyDirection::Press, both);
            h.key_mods(0x43, KeyDirection::Press, both);
            h.key_mods(0x43, KeyDirection::Release, both);
            h.key_mods(0x2A, KeyDirection::Release, ctrl);
            h.key_mods(0x1D, KeyDirection::Release, none);
        };

        type_word(&h, &GHBDSN);
        h.settle();
        tap_chord(&h);
        h.settle();
        tap_chord(&h);
        h.settle();
        assert_eq!(
            erase_counts(&h).len(),
            2,
            "each tap is its own press: {:?}",
            h.emitter.ops()
        );
    }

    /// The report that reopened #44, replayed as key events rather than
    /// as commands: the chord is what carries the state that can get
    /// stuck, and a command sent straight to the engine skips all of
    /// it. Several switches by hand, the line deleted, new text, and
    /// the gesture again.
    #[test]
    fn the_chord_still_answers_after_switches_a_deletion_and_a_retype() {
        let h = Harness::start(60_000);
        h.cmd_tx
            .send(EngineCommand::SetKeystreamHotkeys(KeystreamHotkeys {
                grabbed: [None, None],
                pause: None,
                switch_last: Some(Binding::Key(Chord {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    meta: false,
                    scancode: 0x43,
                })),
            }))
            .expect("engine alive");

        let none = poltertype_types::Modifiers::NONE;
        let ctrl = poltertype_types::Modifiers {
            control: true,
            ..none
        };
        let both = poltertype_types::Modifiers {
            control: true,
            shift: true,
            ..none
        };
        let tap_chord = |h: &Harness| {
            h.key_mods(0x1D, KeyDirection::Press, ctrl);
            h.key_mods(0x2A, KeyDirection::Press, both);
            h.key_mods(0x43, KeyDirection::Press, both);
            h.key_mods(0x43, KeyDirection::Release, both);
            h.key_mods(0x2A, KeyDirection::Release, ctrl);
            h.key_mods(0x1D, KeyDirection::Release, none);
        };

        type_word(&h, &GHBDSN);
        h.settle();
        for _ in 0..3 {
            tap_chord(&h);
            h.settle();
            // keyd hands our own correction back through the stream,
            // which is the reporter's stack and the half a command-only
            // replay leaves out.
            h.replay_echoes();
            h.settle();
        }
        let before = erase_counts(&h).len();

        for _ in 0..(GHBDSN.len() + 4) {
            h.tap(BACKSPACE);
        }
        h.settle();
        type_word(&h, &GHBDSN);
        h.settle();

        tap_chord(&h);
        h.settle();
        assert_eq!(
            erase_counts(&h).len(),
            before + 1,
            "the chord must still answer: {:?}",
            h.emitter.ops()
        );
    }

    /// The same gesture, but the line is rubbed out *past* what the
    /// buffer ever saw — which is what deleting a line actually looks
    /// like. Reported against 0.25.1: several manual switches, then the
    /// text deleted and retyped, and the hotkey answered nothing.
    #[test]
    fn the_hotkey_still_answers_after_the_line_was_rubbed_out_past_the_buffer() {
        let h = Harness::start(60_000);
        type_word(&h, &GHBDSN);
        h.settle();
        for _ in 0..3 {
            h.cmd_tx
                .send(EngineCommand::SwitchLastForcefully)
                .expect("engine alive");
            h.settle();
        }
        let before = h.switcher.switches.lock().len();

        // Four keystrokes more than the word: the caret is now left of
        // anything this buffer has ever tracked.
        for _ in 0..(GHBDSN.len() + 4) {
            h.tap(BACKSPACE);
        }
        h.settle();

        type_word(&h, &GHBDSN);
        h.settle();
        h.cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        h.settle();

        assert_eq!(
            h.switcher.switches.lock().len(),
            before + 1,
            "the gesture must answer the word typed after a deleted line"
        );
    }

    /// Two presses on a word still being typed put it back where it
    /// started — the rotation is computed from where the word reads
    /// *now*, not from the layout it was first typed in.
    #[test]
    fn a_second_press_on_an_unfinished_word_rotates_it_back() {
        let h = Harness::start(60_000);
        type_word(&h, &GHBDSN);
        h.settle();
        for _ in 0..2 {
            h.cmd_tx
                .send(EngineCommand::SwitchLastForcefully)
                .expect("engine alive");
            h.settle();
        }
        let switches = h.switcher.switches.lock().clone();
        assert_eq!(
            switches,
            vec![LayoutId::from("uk-UA"), LayoutId::from("en-US")],
            "the second press must bring the word back, not re-apply the first"
        );
    }

    /// Issue #39: the force-switch chord held down long enough for the
    /// kernel to autorepeat it.
    ///
    /// evdev reports a held key as repeated presses. Those arrive while
    /// the correction is still being prepared, and any press carrying
    /// Ctrl reads as a shortcut the engine cannot reconstruct — so the
    /// correction the chord had just asked for was abandoned before a
    /// single key went out.
    #[test]
    fn holding_the_force_switch_chord_does_not_abort_its_own_correction() {
        let h = Harness::start(60_000);
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.wait_for(|e| matches!(e, SwitcherEvent::Corrected { .. }));
        h.settle();
        let before = h.emitter.ops().len();

        h.cmd_tx
            .send(EngineCommand::SetKeystreamHotkeys(KeystreamHotkeys {
                grabbed: [None, None],
                pause: None,
                switch_last: Some(Binding::Key(Chord {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    meta: false,
                    scancode: 0x43,
                })),
            }))
            .expect("engine alive");

        let ctrl = poltertype_types::Modifiers {
            control: true,
            ..poltertype_types::Modifiers::NONE
        };
        let both = poltertype_types::Modifiers {
            control: true,
            shift: true,
            ..poltertype_types::Modifiers::NONE
        };
        h.key_mods(0x1D, KeyDirection::Press, ctrl);
        h.key_mods(0x2A, KeyDirection::Press, both);
        h.key_mods(0x43, KeyDirection::Press, both);
        for _ in 0..20 {
            h.key_mods(0x43, KeyDirection::Press, both);
            std::thread::sleep(Duration::from_millis(30));
        }
        h.key_mods(0x43, KeyDirection::Release, both);
        h.key_mods(0x2A, KeyDirection::Release, ctrl);
        h.key_mods(
            0x1D,
            KeyDirection::Release,
            poltertype_types::Modifiers::NONE,
        );
        h.settle();

        let ops = h.emitter.ops();
        let erases: Vec<usize> = ops[before..]
            .iter()
            .filter_map(|o| match o {
                EmitOp::Backspaces(n) => Some(*n),
                _ => None,
            })
            .collect();
        assert_eq!(
            erases,
            vec![GHBDSN.len() + 1],
            "the held chord must undo the correction exactly once — word plus its boundary"
        );
        assert!(
            ops[before..]
                .iter()
                .any(|o| matches!(o, EmitOp::Keys(k) if k.len() == GHBDSN.len() + 1)),
            "and the word must be typed back, not left erased"
        );
    }

    /// Issue #44: leaning on the chord for longer than
    /// `CHORD_RELEASE_WAIT` left the gesture dead — the reporter had to
    /// quit and reopen the app, and even then the next long hold killed
    /// it again.
    #[test]
    fn the_chord_still_answers_after_a_hold_outlasting_the_release_wait() {
        let h = Harness::start(60_000);
        h.cmd_tx
            .send(EngineCommand::SetKeystreamHotkeys(KeystreamHotkeys {
                grabbed: [None, None],
                pause: None,
                switch_last: Some(Binding::Key(Chord {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    meta: false,
                    scancode: 0x43,
                })),
            }))
            .expect("engine alive");

        let ctrl = poltertype_types::Modifiers {
            control: true,
            ..poltertype_types::Modifiers::NONE
        };
        let both = poltertype_types::Modifiers {
            control: true,
            shift: true,
            ..poltertype_types::Modifiers::NONE
        };
        // `hold_ms` of autorepeat, the way a real machine reports a
        // held chord — including what our own emitter does to it.
        //
        // The listener folds modifier *events* into the flags it stamps
        // on every key, and `release_modifiers` puts Ctrl and Shift
        // releases on that same wire. So from the moment a correction
        // gives up waiting, the key the user is still holding repeats
        // with no modifiers on it at all.
        let press_chord = |hold_ms: u64| {
            let released_mods = || {
                h.emitter
                    .ops()
                    .iter()
                    .any(|o| matches!(o, EmitOp::ReleaseModifiers))
            };
            let base = released_mods();
            let mut mods = both;
            h.key_mods(0x1D, KeyDirection::Press, ctrl);
            h.key_mods(0x2A, KeyDirection::Press, both);
            h.key_mods(0x43, KeyDirection::Press, mods);
            let until = Instant::now() + Duration::from_millis(hold_ms);
            while Instant::now() < until {
                std::thread::sleep(Duration::from_millis(30));
                if !base && released_mods() {
                    mods = poltertype_types::Modifiers::NONE;
                }
                h.key_mods(0x43, KeyDirection::Press, mods);
            }
            h.key_mods(0x43, KeyDirection::Release, mods);
            h.key_mods(
                0x2A,
                KeyDirection::Release,
                poltertype_types::Modifiers::NONE,
            );
            h.key_mods(
                0x1D,
                KeyDirection::Release,
                poltertype_types::Modifiers::NONE,
            );
        };

        type_word(&h, &GHBDSN);
        h.settle();

        press_chord(0);
        h.settle();
        let after_first = erase_counts(&h).len();
        assert_eq!(after_first, 1, "a plain press must switch the word");

        press_chord(2_600);
        h.settle();
        let after_hold = erase_counts(&h).len();
        assert_eq!(
            after_hold, 2,
            "the held press must switch it too, once the chord comes up"
        );

        press_chord(0);
        h.settle();
        let erases = erase_counts(&h);
        assert_eq!(
            erases.len(),
            3,
            "and the gesture must still answer afterwards: {erases:?}"
        );
        assert_eq!(
            erases.last(),
            Some(&GHBDSN.len()),
            "acting on the word under the caret, not on a stale one: {erases:?}"
        );

        // The word behind the caret has to have survived the hold too.
        // Repeats land inside the correction's own probes as well, and
        // read there as the user reaching for a nav key they abandoned
        // the buffer: the six characters on screen stopped being
        // anything the engine knew, so the next press acted on the tail.
        let tail = [0x18u32, 0x1F];
        for sc in tail {
            h.tap(sc);
        }
        h.settle();
        press_chord(0);
        h.settle();
        assert_eq!(
            erase_counts(&h).last(),
            Some(&(GHBDSN.len() + tail.len())),
            "the whole word must still be the engine's: {:?}",
            erase_counts(&h)
        );
    }

    /// Past the wait, a correction does not happen at all — because
    /// there is nowhere for it to go.
    ///
    /// Neither desktop delivers a keystroke of ours anywhere useful
    /// while the key that asked for it is down: X11 hands everything to
    /// the client holding the grab, and on Wayland libinput drops the
    /// modifier release we send from a device that never pressed it, so
    /// the burst lands as `Ctrl+H`, `Ctrl+G`, `Ctrl+B`. That is what
    /// issue #44 actually put into the reporter's window. Leaving the
    /// word as typed is the only outcome that cannot make it worse.
    #[test]
    fn a_hold_outlasting_the_wait_types_nothing_at_all() {
        let h = Harness::start(60_000);
        h.cmd_tx
            .send(EngineCommand::SetKeystreamHotkeys(KeystreamHotkeys {
                grabbed: [None, None],
                pause: None,
                switch_last: Some(Binding::Key(Chord {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    meta: false,
                    scancode: 0x43,
                })),
            }))
            .expect("engine alive");

        let ctrl = poltertype_types::Modifiers {
            control: true,
            ..poltertype_types::Modifiers::NONE
        };
        let both = poltertype_types::Modifiers {
            control: true,
            shift: true,
            ..poltertype_types::Modifiers::NONE
        };

        type_word(&h, &GHBDSN);
        h.settle();

        h.key_mods(0x1D, KeyDirection::Press, ctrl);
        h.key_mods(0x2A, KeyDirection::Press, both);
        h.key_mods(0x43, KeyDirection::Press, both);
        let until = Instant::now() + CHORD_RELEASE_WAIT + Duration::from_millis(600);
        while Instant::now() < until {
            std::thread::sleep(Duration::from_millis(30));
            h.key_mods(0x43, KeyDirection::Press, both);
        }
        assert!(
            erase_counts(&h).is_empty(),
            "nothing may be emitted while the key is still down: {:?}",
            h.emitter.ops()
        );

        h.key_mods(0x43, KeyDirection::Release, both);
        h.key_mods(
            0x2A,
            KeyDirection::Release,
            poltertype_types::Modifiers::NONE,
        );
        h.key_mods(
            0x1D,
            KeyDirection::Release,
            poltertype_types::Modifiers::NONE,
        );
        h.settle();

        // And the gesture is still there to be used, on the word that
        // is still exactly as it was typed.
        h.key_mods(0x1D, KeyDirection::Press, ctrl);
        h.key_mods(0x2A, KeyDirection::Press, both);
        h.key_mods(0x43, KeyDirection::Press, both);
        h.key_mods(
            0x43,
            KeyDirection::Release,
            poltertype_types::Modifiers::NONE,
        );
        h.key_mods(
            0x2A,
            KeyDirection::Release,
            poltertype_types::Modifiers::NONE,
        );
        h.key_mods(
            0x1D,
            KeyDirection::Release,
            poltertype_types::Modifiers::NONE,
        );
        h.settle();
        assert_eq!(
            erase_counts(&h),
            vec![GHBDSN.len()],
            "one correction, and it is the one asked for after the key came up"
        );
    }

    /// A hotkey with no modifier on it — a bare function key, or the
    /// Caps Lock people ask for by name — used to work exactly once per
    /// word.
    ///
    /// Its own press reaches the buffer like any other key, and the
    /// classifier reads the function row as navigation: the cursor has
    /// moved, forget everything. So the press that had just switched a
    /// word threw that word away on its way out, and the next press
    /// found nothing to act on.
    #[test]
    fn a_hotkey_with_no_modifier_does_not_throw_away_the_word_it_switched() {
        let h = Harness::start(60_000);
        h.cmd_tx
            .send(EngineCommand::SetKeystreamHotkeys(KeystreamHotkeys {
                grabbed: [None, None],
                pause: None,
                switch_last: Some(Binding::Key(Chord {
                    ctrl: false,
                    shift: false,
                    alt: false,
                    meta: false,
                    scancode: 0x43,
                })),
            }))
            .expect("engine alive");

        type_word(&h, &GHBDSN);
        h.settle();
        for _ in 0..2 {
            h.tap(0x43);
            h.settle();
        }

        assert_eq!(
            erase_counts(&h),
            vec![GHBDSN.len(), GHBDSN.len()],
            "both presses must act on the word that is still on screen"
        );
    }

    /// A press that ends up doing nothing must not eat the word it was
    /// going to act on.
    ///
    /// The stash is *taken* to serve a press, and a hold longer than
    /// the wait ends with nothing typed — so without putting it back
    /// the gesture works once and then finds nothing, which is the
    /// symptom of #44 wearing a different cause. Only visible on a
    /// finished word: one still being typed is read straight out of
    /// the buffer and was never taken from anywhere. Caught by the
    /// desktop matrix, not by the first test.
    #[test]
    fn a_hold_that_ends_in_nothing_leaves_the_stashed_word_alone() {
        let h = Harness::start(60_000);
        h.cmd_tx
            .send(EngineCommand::SetKeystreamHotkeys(KeystreamHotkeys {
                grabbed: [None, None],
                pause: None,
                switch_last: Some(Binding::Key(Chord {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    meta: false,
                    scancode: 0x43,
                })),
            }))
            .expect("engine alive");

        let ctrl = poltertype_types::Modifiers {
            control: true,
            ..poltertype_types::Modifiers::NONE
        };
        let both = poltertype_types::Modifiers {
            control: true,
            shift: true,
            ..poltertype_types::Modifiers::NONE
        };
        let press_chord = |hold_ms: u64| {
            h.key_mods(0x1D, KeyDirection::Press, ctrl);
            h.key_mods(0x2A, KeyDirection::Press, both);
            h.key_mods(0x43, KeyDirection::Press, both);
            let until = Instant::now() + Duration::from_millis(hold_ms);
            while Instant::now() < until {
                std::thread::sleep(Duration::from_millis(30));
                h.key_mods(0x43, KeyDirection::Press, both);
            }
            h.key_mods(0x43, KeyDirection::Release, both);
            h.key_mods(
                0x2A,
                KeyDirection::Release,
                poltertype_types::Modifiers::NONE,
            );
            h.key_mods(
                0x1D,
                KeyDirection::Release,
                poltertype_types::Modifiers::NONE,
            );
            h.settle();
        };

        // A *finished* word: the boundary is what puts it on the stash.
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.wait_for(|e| matches!(e, SwitcherEvent::Corrected { .. }));
        h.settle();
        let before = erase_counts(&h).len();

        press_chord(CHORD_RELEASE_WAIT.as_millis() as u64 + 600);
        assert_eq!(
            erase_counts(&h).len(),
            before,
            "the hold outlasted the wait, so nothing may have been typed"
        );

        press_chord(0);
        // Counted, not just read off the end: the automatic correction
        // at the boundary erased exactly the same word and boundary, so
        // the last value alone is true whether or not this press did
        // anything at all.
        let erases = erase_counts(&h);
        assert_eq!(
            erases.len(),
            before + 1,
            "the next press must find the word still stashed: {erases:?}"
        );
        assert_eq!(
            erases.last(),
            Some(&(GHBDSN.len() + 1)),
            "and act on the word plus its boundary: {erases:?}"
        );
    }

    /// Issue #40 as it was actually reported: through the chord, not
    /// through the command channel.
    ///
    /// The chord's own key carries Ctrl, and `handle_key` reads any
    /// press carrying Ctrl as a shortcut that may have edited the text
    /// — so the correction settled the buffer and the very same key
    /// event then threw it away again. Only the modifier-only binding
    /// escaped it, which is why the first regression net missed this.
    #[test]
    fn the_chord_itself_leaves_the_word_it_just_switched_alone() {
        let h = Harness::start(60_000);
        h.cmd_tx
            .send(EngineCommand::SetKeystreamHotkeys(KeystreamHotkeys {
                grabbed: [None, None],
                pause: None,
                switch_last: Some(Binding::Key(Chord {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    meta: false,
                    scancode: 0x43,
                })),
            }))
            .expect("engine alive");

        let ctrl = poltertype_types::Modifiers {
            control: true,
            ..poltertype_types::Modifiers::NONE
        };
        let both = poltertype_types::Modifiers {
            control: true,
            shift: true,
            ..poltertype_types::Modifiers::NONE
        };
        let press_chord = || {
            h.key_mods(0x1D, KeyDirection::Press, ctrl);
            h.key_mods(0x2A, KeyDirection::Press, both);
            std::thread::sleep(Duration::from_millis(120));
            h.key_mods(0x43, KeyDirection::Press, both);
            h.key_mods(0x43, KeyDirection::Release, both);
            h.key_mods(0x2A, KeyDirection::Release, ctrl);
            h.key_mods(
                0x1D,
                KeyDirection::Release,
                poltertype_types::Modifiers::NONE,
            );
        };

        // A word with no separator after it, switched out and back.
        type_word(&h, &GHBDSN);
        h.settle();
        press_chord();
        h.settle();
        press_chord();
        h.settle();

        // Rub it out, type a shorter one, and ask again.
        for _ in 0..GHBDSN.len() {
            h.tap(BACKSPACE);
        }
        let second = [0x32u32, 0x18, 0x18];
        for sc in second {
            h.tap(sc);
        }
        h.settle();
        press_chord();
        h.settle();

        let erases: Vec<usize> = h
            .emitter
            .ops()
            .iter()
            .filter_map(|o| match o {
                EmitOp::Backspaces(n) => Some(*n),
                _ => None,
            })
            .collect();
        assert_eq!(
            erases.last(),
            Some(&second.len()),
            "the third press must act on the word now under the caret: {erases:?}"
        );
    }

    /// The same hold, where an OS-level grab owns the chord and the
    /// engine matches nothing itself.
    ///
    /// The grab hides the chord from our matcher, not the key from our
    /// listener — and on X11 it keeps the whole keyboard grabbed while
    /// the key is down, so everything the correction emits goes to the
    /// grabbing client instead of to the application. Measured on
    /// IceWM, 2026-08-28: the deletion deleted nothing and the replay
    /// typed nothing.
    #[test]
    fn a_grabbed_chord_held_down_is_waited_out_rather_than_typed_over() {
        let h = Harness::start(60_000);
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.wait_for(|e| matches!(e, SwitcherEvent::Corrected { .. }));
        h.settle();
        let before = h.emitter.ops().len();

        h.cmd_tx
            .send(EngineCommand::SetKeystreamHotkeys(KeystreamHotkeys {
                pause: None,
                switch_last: None,
                grabbed: [
                    None,
                    Some(Chord {
                        ctrl: true,
                        shift: true,
                        alt: false,
                        meta: false,
                        scancode: 0x43,
                    }),
                ],
            }))
            .expect("engine alive");

        let ctrl = poltertype_types::Modifiers {
            control: true,
            ..poltertype_types::Modifiers::NONE
        };
        let both = poltertype_types::Modifiers {
            control: true,
            shift: true,
            ..poltertype_types::Modifiers::NONE
        };
        h.key_mods(0x1D, KeyDirection::Press, ctrl);
        h.key_mods(0x2A, KeyDirection::Press, both);
        // Let the run loop take the modifiers before the chord fires:
        // a person's fingers arrive in that order, and a correction
        // that finds a bare Ctrl press still queued reads it as a
        // shortcut it cannot reconstruct — which is a different bug.
        wait_until_taken(&h);
        h.key_mods(0x43, KeyDirection::Press, both);
        // The grab delivered the chord; the key events reach us too.
        h.cmd_tx
            .send(EngineCommand::SwitchLastForcefully)
            .expect("engine alive");
        // Long enough to outlast the absorb window the correction
        // waits on anyway — otherwise "nothing emitted yet" is true
        // whether or not the chord is being waited for. Held to a
        // wall-clock deadline rather than a count of sleeps: the sleeps
        // stretch on a loaded machine, and a hold that stretches past
        // `CHORD_RELEASE_WAIT` gives up on the correction — which is
        // correct behaviour reported as a failed assertion (macOS CI,
        // 2026-08-30).
        let held_until = Instant::now() + Duration::from_millis(900);
        while Instant::now() < held_until {
            h.key_mods(0x43, KeyDirection::Press, both);
            std::thread::sleep(Duration::from_millis(30));
        }
        // Nothing may have gone out yet: everything we emit while the
        // grab is active is delivered to the grabbing client instead
        // of to the application, so a burst here is a burst thrown
        // away — and the word is left looking uncorrected.
        assert_eq!(
            h.emitter.ops().len(),
            before,
            "nothing may be emitted while the chord is still held: {:?}",
            &h.emitter.ops()[before..]
        );
        // No release for the grabbed key, ever — and that is not the
        // harness being lazy. The grab is *active* from the press, and
        // from then on the key's raw events go to the grabbing client
        // alone: the press arrives here, the release does not.
        // Measured on Cinnamon X11, 2026-08-29. So the modifiers
        // coming up is the whole of what says the chord was let go.
        h.key_mods(0x2A, KeyDirection::Release, ctrl);
        h.key_mods(
            0x1D,
            KeyDirection::Release,
            poltertype_types::Modifiers::NONE,
        );
        h.settle();

        let ops = h.emitter.ops();
        let erases: Vec<usize> = ops[before..]
            .iter()
            .filter_map(|o| match o {
                EmitOp::Backspaces(n) => Some(*n),
                _ => None,
            })
            .collect();
        assert_eq!(
            erases,
            vec![GHBDSN.len() + 1],
            "the held chord must undo the correction exactly once"
        );
    }
}

mod boundary_tests {
    use super::{is_structural_boundary, is_submission_boundary, looks_like_all_caps};

    #[test]
    fn flags_url_path_email_chars() {
        for c in [':', '/', '\\', '@', '=', '#', '&'] {
            assert!(is_structural_boundary(c), "expected {c:?} structural");
        }
    }

    #[test]
    fn ignores_natural_prose_punctuation() {
        for c in [
            ' ', '\t', '\n', '.', ',', ';', '!', '?', '(', ')', '"', '\'',
        ] {
            assert!(
                !is_structural_boundary(c),
                "expected {c:?} natural-prose punctuation"
            );
        }
    }

    #[test]
    fn submission_boundary_flags_enter_and_tab() {
        for c in ['\n', '\r', '\t'] {
            assert!(is_submission_boundary(c), "expected {c:?} submission");
        }
    }

    /// Space and ordinary punctuation are safe to re-emit, so
    /// auto-correct must still fire on them.
    #[test]
    fn submission_boundary_ignores_space_and_punctuation() {
        for c in [' ', '.', ',', ';', '!', '?', ':', '/'] {
            assert!(
                !is_submission_boundary(c),
                "expected {c:?} not a submission boundary"
            );
        }
    }

    /// Switching `URL` because it looks like a Cyrillic noun under uk-UA
    /// is exactly what this filter exists to stop, in either script.
    #[test]
    fn all_caps_flags_latin_and_cyrillic_abbreviations() {
        for w in ["URL", "HTTP", "API", "OK", "IP", "ССЫЛКА", "АПІ"] {
            assert!(looks_like_all_caps(w), "expected `{w}` to look ALL CAPS");
        }
    }

    /// Lone uppercase letters are ambiguous: a sentence-initial Shift
    /// looks identical to the pronoun `I`.
    #[test]
    fn all_caps_ignores_single_uppercase_letter() {
        for w in ["I", "A", "Я", "Є"] {
            assert!(
                !looks_like_all_caps(w),
                "single-letter `{w}` is ambiguous — must not be flagged"
            );
        }
    }

    /// Any lowercase letter disqualifies the buffer: that is prose with
    /// a Shift for the initial, and the detector should run as usual.
    /// `iPhone` / `IPv4` mix case on purpose and fall through too.
    #[test]
    fn all_caps_rejects_mixed_and_lowercase() {
        for w in [
            "hello",
            "Hello",
            "Привіт",
            "iPhone",
            "IPv4",
            "PostgreSQL",
            "ім'я",
        ] {
            assert!(
                !looks_like_all_caps(w),
                "mixed-case / lowercase `{w}` must not be flagged"
            );
        }
    }

    /// Digits and the in-word apostrophe live in the buffer alongside
    /// real letters (see `is_word_char`) but are case-less, so they must
    /// not tip the verdict either way.
    #[test]
    fn all_caps_treats_digits_and_apostrophe_as_neutral() {
        assert!(looks_like_all_caps("URL2"));
        assert!(looks_like_all_caps("DON'T"));
        assert!(!looks_like_all_caps("1234"));
        assert!(!looks_like_all_caps("'"));
    }

    /// Defensive: `decide` short-circuits before an empty buffer gets
    /// here, but the helper must not claim "yes" by vacuous truth.
    #[test]
    fn all_caps_rejects_empty_string() {
        assert!(!looks_like_all_caps(""));
    }
}

mod force_switch_rearm_tests {
    use crate::engine::consts::FORCE_SWITCH_REARM;
    use std::time::Duration;

    /// Regression for the manual-switch hotkey loop.
    ///
    /// `force_switch_last` emits Backspaces flagged injected, but Win32
    /// `RegisterHotKey` sees them combined with the user's still-held
    /// Ctrl+Shift as a fresh press and fires again; auto-repeat does
    /// the same. That used to be absorbed by taking the stash
    /// atomically, which also made the hotkey work exactly once per
    /// word (issue #37). The stash is put back now, so this window is
    /// the whole of the guard: wide enough to cover an echo, narrow
    /// enough to let a person press again.
    #[test]
    fn the_rearm_window_separates_an_echo_from_a_second_press() {
        // The echo is queued while we are still injecting and handled
        // microseconds later; a held key repeats far faster still.
        assert!(
            FORCE_SWITCH_REARM >= Duration::from_millis(100),
            "too narrow to cover the echo our own Backspaces provoke — \
             if this regresses, the hotkey loop bug is back"
        );
        // A person has to see the result before pressing again.
        assert!(
            FORCE_SWITCH_REARM <= Duration::from_millis(250),
            "wide enough to swallow a deliberate second press"
        );
    }
}

mod code_check_render_tests {
    use super::render_for_code_check;
    use crate::layouts::LayoutDb;
    use poltertype_layout::LayoutId;
    use poltertype_types::WordKey;

    fn k(scancode: u32, shift: bool) -> WordKey {
        WordKey {
            scancode,
            shift,
            caps: false,
            timestamp_ms: 0,
        }
    }

    /// Regression: `Друже` typed while en-US is active renders `Lhe;t`
    /// (0x27, the uk-UA letter `ж`, is `;` under en-US), and that bare
    /// `;` made `looks_like_code_token` veto the auto-switch.
    #[test]
    fn strips_cross_layout_punct_from_render() {
        let db = LayoutDb::load_embedded();
        let en = LayoutId::from("en-US");
        // Scancodes for `Друже` in uk-UA — same physical keys as
        // `L`, `h`, `e`, `;`, `t` in en-US.
        let keys = vec![
            k(0x26, true),  // Д / L
            k(0x23, false), // р / h
            k(0x12, false), // у / e
            k(0x27, false), // ж / ;
            k(0x14, false), // е / t
        ];
        let cleaned = render_for_code_check(&keys, &en, &db, "Lhe;t");
        assert_eq!(cleaned, "Lhet");
    }

    /// A real `_` (0x0C with shift) is `_` in both layouts and a letter
    /// in neither, so it must survive the cleanup — otherwise the
    /// snake_case heuristic stops firing on real code.
    #[test]
    fn keeps_genuine_underscore() {
        let db = LayoutDb::load_embedded();
        let en = LayoutId::from("en-US");
        // `foo_bar` scancodes under en-US.
        let keys = vec![
            k(0x21, false), // f
            k(0x18, false), // o
            k(0x18, false), // o
            k(0x0C, true),  // _
            k(0x30, false), // b
            k(0x1E, false), // a
            k(0x13, false), // r
        ];
        let cleaned = render_for_code_check(&keys, &en, &db, "foo_bar");
        assert_eq!(cleaned, "foo_bar");
    }

    /// Sanity: under uk-UA, the same `Друже` scancodes render as
    /// pure letters; nothing to strip.
    #[test]
    fn cyrillic_render_unchanged() {
        let db = LayoutDb::load_embedded();
        let uk = LayoutId::from("uk-UA");
        let keys = vec![
            k(0x26, true),  // Д
            k(0x23, false), // р
            k(0x12, false), // у
            k(0x27, false), // ж
            k(0x14, false), // е
        ];
        let cleaned = render_for_code_check(&keys, &uk, &db, "Друже");
        assert_eq!(cleaned, "Друже");
    }

    /// A current layout missing from the DB returns `fallback`
    /// untouched, so the mid-decision path can continue.
    #[test]
    fn falls_back_when_layout_missing() {
        let db = LayoutDb::load_embedded();
        let nonexistent = LayoutId::from("xx-YY");
        let cleaned = render_for_code_check(&[], &nonexistent, &db, "fallback");
        assert_eq!(cleaned, "fallback");
    }
}

mod boundary_key_tests {
    use super::boundary_key_for;
    use crate::layouts::LayoutDb;
    use poltertype_layout::LayoutId;

    /// The reported bug: `,` lives on `Shift`+`0x35` in uk-UA and on a
    /// bare `0x33` in en-US, so replaying the key as typed turned the
    /// comma that closed a corrected word into `?`.
    #[test]
    fn comma_moves_to_the_targets_own_key() {
        let db = LayoutDb::load_embedded();
        assert_eq!(
            boundary_key_for(&db, &LayoutId::from("en-US"), 0x35, true, ','),
            (0x33, false)
        );
        // …and back the other way, for a word corrected into uk-UA.
        assert_eq!(
            boundary_key_for(&db, &LayoutId::from("uk-UA"), 0x33, false, ','),
            (0x35, true)
        );
    }

    /// The dot is on `0x35` unshifted in uk-UA and on `0x34` in en-US —
    /// the same trap, one key over.
    #[test]
    fn dot_moves_too() {
        let db = LayoutDb::load_embedded();
        assert_eq!(
            boundary_key_for(&db, &LayoutId::from("en-US"), 0x35, false, '.'),
            (0x34, false)
        );
    }

    /// A character the target produces on the very key that was typed
    /// keeps it, rather than wandering to another key carrying the same
    /// glyph.
    #[test]
    fn key_is_kept_when_the_target_agrees() {
        let db = LayoutDb::load_embedded();
        assert_eq!(
            boundary_key_for(&db, &LayoutId::from("en-US"), 0x35, true, '?'),
            (0x35, true)
        );
    }

    /// Space, Enter and Tab are in no mapping table at all; they are
    /// the same physical key everywhere and must pass through.
    #[test]
    fn layout_independent_keys_pass_through() {
        let db = LayoutDb::load_embedded();
        let en = LayoutId::from("en-US");
        assert_eq!(boundary_key_for(&db, &en, 0x39, false, ' '), (0x39, false));
        assert_eq!(boundary_key_for(&db, &en, 0x1C, false, '\n'), (0x1C, false));
        assert_eq!(boundary_key_for(&db, &en, 0x0F, false, '\t'), (0x0F, false));
    }

    /// Nothing to remap to (unknown layout, or a character the target
    /// cannot type) leaves the key as it was: the correction is still
    /// worth making with the wrong separator.
    #[test]
    fn falls_back_to_the_typed_key() {
        let db = LayoutDb::load_embedded();
        assert_eq!(
            boundary_key_for(&db, &LayoutId::from("xx-YY"), 0x35, true, ','),
            (0x35, true)
        );
        assert_eq!(
            boundary_key_for(&db, &LayoutId::from("en-US"), 0x35, true, 'ї'),
            (0x35, true)
        );
    }

    /// Every bundled layout can type the two separators that close
    /// almost every word — otherwise the fallback above quietly becomes
    /// the normal path for that language.
    ///
    /// Deliberately just these two: the bundled tables cover the plain
    /// and shift levels only, and a few layouts reach some punctuation
    /// through AltGr, which PolterType does not track at all (bg-BG has
    /// no `(`, pt-BR no `?`). Those fall back to the key as typed.
    #[test]
    fn every_bundled_layout_can_type_a_full_stop_and_a_comma() {
        let db = LayoutDb::load_embedded();
        for (id, mapping) in db.iter() {
            for ch in ['.', ','] {
                assert!(
                    mapping.key_for_char(ch).is_some(),
                    "{id} cannot type {ch:?}"
                );
            }
        }
    }
}

mod layout_eligibility_tests {
    use super::is_layout_eligible;
    use poltertype_layout::LayoutId;

    fn id(s: &str) -> LayoutId {
        LayoutId::from(s)
    }

    /// The original "http " bug: the detector picked `fr-FR` with only
    /// en-US / ru-RU / uk-UA active in the OS, and `switch_to(fr-FR)`
    /// then aborted *after* backspaces had destroyed the word.
    #[test]
    fn os_inactive_layout_is_dropped_from_candidates() {
        let current = id("uk-UA");
        let os_active = vec![id("en-US"), id("ru-RU"), id("uk-UA")];
        let settings_active: Vec<LayoutId> = vec![]; // empty = "all loaded"
        let settings_ignored: Vec<LayoutId> = vec![];

        // fr-FR is in LayoutDb but NOT in the OS-active list.
        assert!(
            !is_layout_eligible(
                &id("fr-FR"),
                &current,
                &settings_active,
                &settings_ignored,
                Some(&os_active),
            ),
            "fr-FR must be filtered out — user can't switch to a layout they don't have"
        );
        // en-US is OS-active and not blocked → eligible.
        assert!(is_layout_eligible(
            &id("en-US"),
            &current,
            &settings_active,
            &settings_ignored,
            Some(&os_active),
        ));
    }

    /// The current layout always passes, even when the OS list
    /// transiently omits it: a query race would otherwise strip the
    /// layout the user is *currently typing in* from the candidate set,
    /// leaving nothing to render the buffer with.
    #[test]
    fn current_layout_always_passes() {
        let current = id("uk-UA");
        let os_active = vec![id("en-US")]; // uk-UA missing
        assert!(is_layout_eligible(
            &current,
            &current,
            &[],
            &[],
            Some(&os_active),
        ));
    }

    /// A failed OS query (`None`) fails open, leaving settings as the
    /// only filter: occasionally picking an unreachable layout (caught
    /// by the `apply_correction` pre-flight) beats freezing the engine.
    #[test]
    fn fail_open_when_os_query_unavailable() {
        let current = id("uk-UA");
        assert!(is_layout_eligible(&id("fr-FR"), &current, &[], &[], None,));
    }

    /// Settings `ignored` wins over OS-active: a layout the user
    /// disabled stays disabled whatever the OS reports.
    #[test]
    fn ignored_wins_over_os_active() {
        let current = id("uk-UA");
        let os_active = vec![id("en-US"), id("uk-UA"), id("ru-RU")];
        let ignored = vec![id("ru-RU")];
        assert!(!is_layout_eligible(
            &id("ru-RU"),
            &current,
            &[],
            &ignored,
            Some(&os_active),
        ));
    }

    /// Settings allow-list narrows further on top of OS-active.
    #[test]
    fn allow_list_narrows_os_active() {
        let current = id("uk-UA");
        let os_active = vec![id("en-US"), id("uk-UA"), id("ru-RU")];
        let allow = vec![id("en-US"), id("uk-UA")]; // ru-RU not whitelisted
        assert!(!is_layout_eligible(
            &id("ru-RU"),
            &current,
            &allow,
            &[],
            Some(&os_active),
        ));
        assert!(is_layout_eligible(
            &id("en-US"),
            &current,
            &allow,
            &[],
            Some(&os_active),
        ));
    }
}

mod app_match_tests {
    use super::app_is_disabled;

    #[test]
    fn matches_case_insensitively() {
        let list: Vec<String> = ["Code.exe", "alacritty"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        assert!(app_is_disabled("CODE.EXE", &list));
        assert!(app_is_disabled("code.exe", &list));
        assert!(app_is_disabled("Alacritty", &list));
    }

    #[test]
    fn ignores_unrelated_apps() {
        let list: Vec<String> = ["Code.exe"].iter().map(|s| (*s).to_owned()).collect();
        assert!(!app_is_disabled("notepad.exe", &list));
    }
}

mod chord_tests {
    use super::{Chord, match_chord};
    use poltertype_input::{KeyDirection, KeyEvent, Modifiers};

    const SPACE: u32 = 0x39;
    const CTRL_SHIFT_SPACE: Chord = Chord {
        ctrl: true,
        shift: true,
        alt: false,
        meta: false,
        scancode: SPACE,
    };

    fn ev(scancode: u32, direction: KeyDirection, mods: Modifiers) -> KeyEvent {
        KeyEvent {
            vk: scancode,
            scancode,
            direction,
            modifiers: mods,
            injected: false,
            timestamp_ms: 0,
        }
    }

    fn ctrl_shift() -> Modifiers {
        Modifiers {
            control: true,
            shift: true,
            ..Modifiers::NONE
        }
    }

    #[test]
    fn fires_once_per_press_ignoring_autorepeat() {
        let mut down = false;
        assert!(match_chord(
            &ev(SPACE, KeyDirection::Press, ctrl_shift()),
            CTRL_SHIFT_SPACE,
            &mut down
        ));
        // Autorepeat (press again without release) does NOT fire.
        assert!(!match_chord(
            &ev(SPACE, KeyDirection::Press, ctrl_shift()),
            CTRL_SHIFT_SPACE,
            &mut down
        ));
    }

    #[test]
    fn release_rearms_for_next_press() {
        let mut down = false;
        assert!(match_chord(
            &ev(SPACE, KeyDirection::Press, ctrl_shift()),
            CTRL_SHIFT_SPACE,
            &mut down
        ));
        assert!(!match_chord(
            &ev(SPACE, KeyDirection::Release, ctrl_shift()),
            CTRL_SHIFT_SPACE,
            &mut down
        ));
        assert!(match_chord(
            &ev(SPACE, KeyDirection::Press, ctrl_shift()),
            CTRL_SHIFT_SPACE,
            &mut down
        ));
    }

    #[test]
    fn requires_exact_modifiers() {
        let mut down = false;
        // Extra Alt held → no match.
        let with_alt = Modifiers {
            control: true,
            shift: true,
            alt: true,
            ..Modifiers::NONE
        };
        assert!(!match_chord(
            &ev(SPACE, KeyDirection::Press, with_alt),
            CTRL_SHIFT_SPACE,
            &mut down
        ));
        // Missing Shift → no match.
        let mut down2 = false;
        let ctrl_only = Modifiers {
            control: true,
            ..Modifiers::NONE
        };
        assert!(!match_chord(
            &ev(SPACE, KeyDirection::Press, ctrl_only),
            CTRL_SHIFT_SPACE,
            &mut down2
        ));
    }

    #[test]
    fn other_keys_do_not_disturb_latch() {
        let mut down = false;
        // A different key's events must not flip our latch.
        assert!(!match_chord(
            &ev(0x1E, KeyDirection::Press, ctrl_shift()),
            CTRL_SHIFT_SPACE,
            &mut down
        ));
        assert!(!down);
        assert!(match_chord(
            &ev(SPACE, KeyDirection::Press, ctrl_shift()),
            CTRL_SHIFT_SPACE,
            &mut down
        ));
    }
}

mod mod_chord_tests {
    use super::{MOD_TAP_MAX, ModChord, ModSet, ModTapState, match_mod_chord};
    use poltertype_input::{KeyDirection, KeyEvent, Modifiers};
    use std::time::{Duration, Instant};

    const L_CTRL: u32 = 0x1D;
    const L_SHIFT: u32 = 0x2A;
    const R_SHIFT: u32 = 0x36;
    const L_ALT: u32 = 0x38;
    const KEY_C: u32 = 0x2E;

    const CTRL_SHIFT: ModChord = ModChord {
        mods: ModSet {
            ctrl: true,
            shift: true,
            alt: false,
            meta: false,
        },
        double_tap: false,
    };
    const DOUBLE_SHIFT: ModChord = ModChord {
        mods: ModSet {
            ctrl: false,
            shift: true,
            alt: false,
            meta: false,
        },
        double_tap: true,
    };

    fn ev(scancode: u32, direction: KeyDirection) -> KeyEvent {
        KeyEvent {
            vk: scancode,
            scancode,
            direction,
            // The matcher reads key identity, never the aggregate
            // flags: a modifier's own press reports them differently
            // on every backend.
            modifiers: Modifiers::NONE,
            injected: false,
            timestamp_ms: 0,
        }
    }

    /// Feed a gesture, one `(scancode, direction, offset-ms)` at a
    /// time, and report which steps fired.
    fn run(chord: ModChord, steps: &[(u32, KeyDirection, u64)]) -> Vec<usize> {
        let mut st = ModTapState::default();
        let base = Instant::now();
        steps
            .iter()
            .enumerate()
            .filter(|(_, (sc, dir, at))| {
                match_mod_chord(
                    &ev(*sc, *dir),
                    chord,
                    &mut st,
                    base + Duration::from_millis(*at),
                )
            })
            .map(|(i, _)| i)
            .collect()
    }

    #[test]
    fn fires_when_the_modifiers_come_back_up_and_nothing_else_was_pressed() {
        let fired = run(
            CTRL_SHIFT,
            &[
                (L_CTRL, KeyDirection::Press, 0),
                (L_SHIFT, KeyDirection::Press, 40),
                (L_SHIFT, KeyDirection::Release, 120),
                (L_CTRL, KeyDirection::Release, 140),
            ],
        );
        assert_eq!(fired, vec![3], "only the last release may fire");
    }

    /// The rule the whole design turns on: `Ctrl+C` must stay
    /// `Ctrl+C`, and so must every other shortcut the chord's
    /// modifiers are part of.
    #[test]
    fn a_key_pressed_during_the_hold_makes_it_a_shortcut_not_a_tap() {
        for interloper in [KEY_C, poltertype_types::SC_POINTER_BUTTON] {
            let fired = run(
                CTRL_SHIFT,
                &[
                    (L_CTRL, KeyDirection::Press, 0),
                    (L_SHIFT, KeyDirection::Press, 20),
                    (interloper, KeyDirection::Press, 60),
                    (interloper, KeyDirection::Release, 90),
                    (L_SHIFT, KeyDirection::Release, 120),
                    (L_CTRL, KeyDirection::Release, 130),
                ],
            );
            assert!(fired.is_empty(), "fired on {interloper:#x}");
        }
    }

    #[test]
    fn a_modifier_outside_the_chord_does_not_fire_it() {
        let fired = run(
            CTRL_SHIFT,
            &[
                (L_CTRL, KeyDirection::Press, 0),
                (L_SHIFT, KeyDirection::Press, 20),
                (L_ALT, KeyDirection::Press, 40),
                (L_ALT, KeyDirection::Release, 60),
                (L_SHIFT, KeyDirection::Release, 80),
                (L_CTRL, KeyDirection::Release, 100),
            ],
        );
        assert!(fired.is_empty());
    }

    /// A hold is not a tap. This is what keeps a Shift held for a
    /// capital that never came, or a Shift+click on the platforms
    /// where mouse buttons are invisible to us, from firing.
    #[test]
    fn a_long_hold_is_not_a_tap() {
        let late = MOD_TAP_MAX.as_millis() as u64 + 50;
        let fired = run(
            CTRL_SHIFT,
            &[
                (L_CTRL, KeyDirection::Press, 0),
                (L_SHIFT, KeyDirection::Press, 10),
                (L_SHIFT, KeyDirection::Release, late),
                (L_CTRL, KeyDirection::Release, late + 10),
            ],
        );
        assert!(fired.is_empty());
    }

    #[test]
    fn a_double_tap_needs_both_taps_inside_the_window() {
        let quick = run(
            DOUBLE_SHIFT,
            &[
                (L_SHIFT, KeyDirection::Press, 0),
                (L_SHIFT, KeyDirection::Release, 60),
                (R_SHIFT, KeyDirection::Press, 200),
                (R_SHIFT, KeyDirection::Release, 260),
            ],
        );
        assert_eq!(quick, vec![3], "left and right Shift are one modifier");

        let slow = run(
            DOUBLE_SHIFT,
            &[
                (L_SHIFT, KeyDirection::Press, 0),
                (L_SHIFT, KeyDirection::Release, 60),
                (L_SHIFT, KeyDirection::Press, 2_000),
                (L_SHIFT, KeyDirection::Release, 2_060),
            ],
        );
        assert!(slow.is_empty(), "the second tap came too late to pair");
    }

    /// Typing capitals is a Shift hold with a letter in it, which the
    /// dirty rule kills — including the letter's own Shift+release
    /// ordering, where the letter comes up after the modifier.
    #[test]
    fn typing_capitals_never_fires_a_double_shift_binding() {
        let fired = run(
            DOUBLE_SHIFT,
            &[
                (L_SHIFT, KeyDirection::Press, 0),
                (KEY_C, KeyDirection::Press, 30),
                (L_SHIFT, KeyDirection::Release, 60),
                (KEY_C, KeyDirection::Release, 80),
                (L_SHIFT, KeyDirection::Press, 200),
                (KEY_C, KeyDirection::Press, 230),
                (L_SHIFT, KeyDirection::Release, 260),
                (KEY_C, KeyDirection::Release, 280),
            ],
        );
        assert!(fired.is_empty());
    }
}

mod paste_shortcut_tests {
    use super::{SC_INSERT, SC_V, is_paste_shortcut};
    use poltertype_input::{KeyDirection, KeyEvent, Modifiers};

    fn ev(scancode: u32, direction: KeyDirection, mods: Modifiers) -> KeyEvent {
        KeyEvent {
            vk: scancode,
            scancode,
            direction,
            modifiers: mods,
            injected: false,
            timestamp_ms: 0,
        }
    }

    fn ctrl() -> Modifiers {
        Modifiers {
            control: true,
            ..Modifiers::NONE
        }
    }

    #[test]
    fn detects_ctrl_v_and_ctrl_shift_v() {
        assert!(is_paste_shortcut(&ev(SC_V, KeyDirection::Press, ctrl())));
        let ctrl_shift = Modifiers {
            control: true,
            shift: true,
            ..Modifiers::NONE
        };
        assert!(is_paste_shortcut(&ev(
            SC_V,
            KeyDirection::Press,
            ctrl_shift
        )));
    }

    #[test]
    fn detects_shift_insert() {
        let shift = Modifiers {
            shift: true,
            ..Modifiers::NONE
        };
        assert!(is_paste_shortcut(&ev(
            SC_INSERT,
            KeyDirection::Press,
            shift
        )));
    }

    #[test]
    fn ignores_release_edge() {
        assert!(!is_paste_shortcut(&ev(SC_V, KeyDirection::Release, ctrl())));
    }

    #[test]
    fn ignores_plain_v_and_other_ctrl_combos() {
        assert!(!is_paste_shortcut(&ev(
            SC_V,
            KeyDirection::Press,
            Modifiers::NONE
        )));
        let ctrl_c = 0x2E; // SC1 / evdev KEY_C
        assert!(!is_paste_shortcut(&ev(ctrl_c, KeyDirection::Press, ctrl())));
    }

    #[test]
    fn ctrl_alt_v_is_not_paste() {
        // AltGr+V (Ctrl+Alt) is a dead-key / compose combo on some
        // layouts, not a paste — the alt veto keeps it out.
        let ctrl_alt = Modifiers {
            control: true,
            alt: true,
            ..Modifiers::NONE
        };
        assert!(!is_paste_shortcut(&ev(SC_V, KeyDirection::Press, ctrl_alt)));
    }
}
