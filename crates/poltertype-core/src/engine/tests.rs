//! Engine unit + integration tests, split out of `mod.rs`.
//!
//! This prelude re-imports the engine's public API plus the internal
//! submodules, so the inner test modules resolve names through
//! `use super::*` exactly as they did when they lived inline.

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

    /// Records every operation and mimics the uinput emitter's echo
    /// log (press+release per backspace / replay key, shift presses
    /// included) so tests can replay realistic keyd-style echoes.
    /// `emitted` is drained by the engine's `take_emitted`; the test
    /// keeps its own copy in `echo_copy` for replaying.
    #[derive(Default)]
    struct MockEmitter {
        ops: Mutex<Vec<EmitOp>>,
        emitted: Mutex<Vec<EmittedKey>>,
        echo_copy: Mutex<Vec<EmittedKey>>,
        /// Called from `send_keys` once the burst is on the wire: a
        /// test's stand-in for a physical keystroke the compositor
        /// interleaves with our replay.
        during_replay: Mutex<Option<ReplayHook>>,
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
    }

    impl MockSwitcher {
        fn new(current: &str, active: &[&str]) -> Self {
            Self {
                current: Mutex::new(LayoutId::from(current)),
                active: active.iter().map(|s| LayoutId::from(*s)).collect(),
                switches: Mutex::new(Vec::new()),
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
            *self.current.lock() = id.clone();
            Ok(())
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
            )
        }

        /// The widest constructor: `tweak` gets the whole `Settings`
        /// before the engine starts, for the settings no narrower
        /// parameter covers (`[exceptions]`, and whatever comes next).
        fn start_configured(
            idle_timeout_ms: u64,
            emitter: MockEmitter,
            fail_switch: bool,
            suggester: Option<Arc<dyn poltertype_detect::SuggestionProvider>>,
            detectors_override: Option<Vec<Box<dyn Detector>>>,
            accept_modifiers: Option<&str>,
            tweak: impl FnOnce(&mut crate::settings::Settings),
        ) -> Self {
            let mut settings = crate::settings::Settings::default();
            settings.engine.idle_timeout_ms = idle_timeout_ms;
            if let Some(m) = accept_modifiers {
                settings.suggestions.accept_modifiers = m.to_owned();
            }
            tweak(&mut settings);
            let settings = Arc::new(SettingsStore::for_tests(settings));
            let layouts = Arc::new(LayoutDb::load_embedded());
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
                // The gate is a no-op in tests: these exercise the
                // path taken when keystrokes cannot be held back.
                key_gate: poltertype_input::KeyGate::disabled(),
                focus_tracker: Arc::new(NoopFocusTracker),
                audio: Arc::new(crate::audio::AudioPlayer::for_tests()),
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
            }
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
        /// dawdle (quiet-gap absorption, the echo-settle wait, chained
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

        /// Wait until the emitter has recorded at least `n` operations
        /// — i.e. a correction's emission has actually happened. Used
        /// to time echo replays realistically (echoes arrive while the
        /// engine is still inside its post-replay settle window, not
        /// seconds later).
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

    fn type_word(h: &Harness, scancodes: &[u32]) {
        for &sc in scancodes {
            h.tap(sc);
        }
    }

    /// The real pipeline the app wires up: dictionary first,
    /// word-plausibility second. Needed by the domain regression
    /// below — the bug it covers only exists against real scoring.
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

    /// Regression: a domain used to be switched **twice** — once to
    /// mangle the host, then back on the next prose word. `.` is `ю` in
    /// uk-UA, so a host stays one token and its en-US rendering scored
    /// 0.00 against the Cyrillic 0.75. Host and surrounding sentence
    /// must survive, while a genuine wrong-layout word carrying a dot
    /// is still corrected.
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

    /// The guard above must not go so wide that it swallows real
    /// corrections: `союз` typed under en-US comes out as `cj.p` —
    /// dot and all — and still has to be fixed.
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

    /// Baseline: a mistyped word + space triggers exactly one
    /// correction — switch first, then word-length+boundary
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

    /// If the layout switch fails, the correction must abort BEFORE
    /// any backspace reaches the user's text. (The old order deleted
    /// the word first and then discovered the switch was impossible.)
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
        // Echoes arrive promptly (one keyd round-trip), while the
        // engine is still inside its post-replay settle window.
        h.wait_ops(2);
        h.replay_echoes();
        h.settle();
        // Still exactly one correction.
        assert_eq!(h.emitter.ops().len(), 2, "echoes must not re-correct");

        // Buffer unpolluted: the next mistyped word corrects with the
        // right backspace count (its own length + boundary — not more).
        type_word(&h, &GHBDSN); // now typed under uk-UA → detector → en-US
        h.tap(SPACE);
        h.settle();
        let (ops, _) = h.stop();
        assert_eq!(ops[2], EmitOp::Backspaces(7));
    }

    /// СИМПТОМ 2: type a word, complete it, backspace over the space
    /// and two letters, retype them, complete again. The second
    /// correction must cover the WHOLE word (7 backspaces), not just
    /// the retyped tail (3) — under-counting here is what chopped
    /// words in half.
    #[test]
    fn backspace_edit_recorrects_whole_word() {
        let h = Harness::start(60_000);
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        h.wait_ops(2);
        h.replay_echoes(); // keyd delivers our correction's echoes
        h.settle();

        // User edits: BS over the space, BS over the last letter,
        // retype it, complete again.
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

    /// СИМПТОМ 1: the user keeps typing while the correction is in
    /// flight. The raced keystroke is absorbed into the plan before
    /// anything is deleted: one extra backspace, re-typed after the
    /// boundary, and seeded into the next word's buffer.
    #[test]
    fn raced_keystroke_is_compensated() {
        let h = Harness::start(60_000);
        type_word(&h, &GHBDSN);
        h.tap(SPACE);
        // Next word's first letter, already in flight before the
        // engine begins the correction — deterministic because the
        // engine watches the channel for a quiet gap before deleting.
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

        // And it seeds the next word: finish the word with 5 more
        // letters — the next correction must count all 6 + boundary.
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
        // Correction 1 absorbs word2 up to its boundary: deletes
        // word1(6) + space(1) + word2(6) + space(1) = 14, replays all
        // of it in order.
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
        // The resume boundary routed word2 through the normal
        // pipeline; the flip-flop mock detector then corrected it as
        // its own word (7 = 6 keys + boundary).
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

    /// A key that appears nowhere in the correction being replayed.
    /// An intruder sharing a scancode with our own replay can be
    /// swallowed by the echo queue instead — which is a real hazard,
    /// but not the one these tests are about, and it made them depend
    /// on how fast the echoes happened to arrive.
    const INTRUDER: u32 = 0x2D; // `X` — not in GHBDSN, not SPACE

    /// Send one press+release of `sc` into the engine's key stream from
    /// wherever it is called — used to simulate a keystroke the
    /// compositor interleaves with a burst we are still emitting.
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
    /// arrives as a shortcut and nothing is typed. Confirmed against a
    /// real compositor both ways.
    #[test]
    fn accept_chord_releases_its_own_modifiers_before_typing() {
        // This user's configured chord, which also exercises parsing
        // `Meta` — the half the default `Ctrl+Shift` never touches.
        let h = suggestion_harness_with_chord(Some("Ctrl+Meta"));
        type_word(&h, &HWLLO);
        h.tap(SPACE);
        let _generation = ready_generation(&h);
        // Ctrl+Meta, the modifier half of this user's accept chord.
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

    /// An idle pause mid-word (thinking) must not let the engine
    /// correct the tail it saw afterwards — that used to leave the
    /// word's head behind.
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

    /// The flip side — the main chat-box flow: click into an input
    /// field (nothing mid-flight), type a word in the wrong layout,
    /// hit space. That word must still correct, with exactly its own
    /// length. A click must not cost the user their next correction.
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

    // ─── Spelling suggestions ────────────────────────────────────────

    /// Never has an opinion — leaves every word as typed, so the
    /// suggestions gate is reached on each completed word.
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
        // The escape hatch always closes the list, carrying the typed
        // word so the accept path knows what to add.
        assert_eq!(entries[1].action, SuggestionAction::AddToDictionary);
        assert_eq!(entries[1].text, "hwllo");
        let (ops, _) = h.stop();
        assert!(ops.is_empty(), "an offer alone must not emit keystrokes");
    }

    /// Accepting the add-to-dictionary row must emit the
    /// `AddToDictionary` event and touch nothing on screen.
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
    /// *read* the tooltip past `idle_timeout_ms` voided the offer on the
    /// chord itself.
    ///
    /// Drives word → pause past the timeout → the full chord with
    /// listener-faithful modifier flags. The replacement must land.
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
                // No `ReleaseModifiers` here, unlike the test above:
                // this run lets Ctrl and Shift back up while the
                // correction is still absorbing, so by the time it
                // types there is nothing held to get in the way.
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
    /// word", and for a long time it only silenced the suggestion
    /// tooltip while the correction went ahead regardless. The
    /// detector here switches everything it is shown, so anything
    /// that reaches it corrects — nothing must.
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

    /// The manual hotkey after one of our own corrections puts the
    /// word back — it used to re-apply the same correction, which
    /// deleted the word and retyped it identically — and takes the
    /// rescued word into the user's dictionary, the only route the
    /// auto-correction path has into it.
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

    #[test]
    fn submission_boundary_ignores_space_and_punctuation() {
        // Space and ordinary punctuation are safe to re-emit, so they
        // must NOT be treated as submission boundaries (auto-correct
        // should still fire on them).
        for c in [' ', '.', ',', ';', '!', '?', ':', '/'] {
            assert!(
                !is_submission_boundary(c),
                "expected {c:?} not a submission boundary"
            );
        }
    }

    /// Real ALL-CAPS abbreviations are exactly what the filter targets:
    /// the user held Shift / Caps Lock and typed a known acronym in
    /// either script. Switching `URL` because it looks like a Cyrillic
    /// noun under uk-UA is the kind of "correction" the user hated, so
    /// the filter must fire for both Latin and Cyrillic variants.
    #[test]
    fn all_caps_flags_latin_and_cyrillic_abbreviations() {
        for w in ["URL", "HTTP", "API", "OK", "IP", "ССЫЛКА", "АПІ"] {
            assert!(looks_like_all_caps(w), "expected `{w}` to look ALL CAPS");
        }
    }

    /// Lone uppercase letters are ambiguous — "I just hit Shift to
    /// start a sentence" looks identical to "I'm typing the pronoun
    /// `I`". Don't fire the suppressor on those.
    #[test]
    fn all_caps_ignores_single_uppercase_letter() {
        for w in ["I", "A", "Я", "Є"] {
            assert!(
                !looks_like_all_caps(w),
                "single-letter `{w}` is ambiguous — must not be flagged"
            );
        }
    }

    /// Any lowercase letter at all disqualifies the buffer: that's
    /// normal prose typing where the user just hit Shift for the
    /// initial / a proper noun, and we should let the detector run as
    /// usual. `iPhone` / `IPv4` mix case on purpose and must fall
    /// through too.
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
    /// real letters (see `is_word_char`). They're case-less, so they
    /// shouldn't tip the verdict either way — `URL2` and `DON'T` are
    /// still ALL CAPS; `1234` and a lone `'` are not (no upper-letter
    /// count).
    #[test]
    fn all_caps_treats_digits_and_apostrophe_as_neutral() {
        assert!(looks_like_all_caps("URL2"));
        assert!(looks_like_all_caps("DON'T"));
        assert!(!looks_like_all_caps("1234"));
        assert!(!looks_like_all_caps("'"));
    }

    /// Empty input is a defensive case — the engine doesn't call us
    /// with an empty buffer (`decide` short-circuits earlier) but the
    /// helper should still return `false` rather than panic or claim
    /// "yes" via vacuous truth.
    #[test]
    fn all_caps_rejects_empty_string() {
        assert!(!looks_like_all_caps(""));
    }
}

mod last_word_consume_tests {
    use super::LastWord;
    use parking_lot::RwLock;
    use poltertype_layout::LayoutId;
    use std::sync::Arc;

    /// Regression for the manual-switch hotkey loop.
    ///
    /// `force_switch_last` emits Backspaces flagged injected, but Win32
    /// `RegisterHotKey` sees them combined with the user's still-held
    /// Ctrl+Shift as a fresh press and fires again; auto-repeat does the
    /// same. Without atomic take-and-clear every echo re-ran the
    /// correction, so the text accumulated and the sound looped until
    /// the app was killed.
    ///
    /// A full `SwitcherEngine` is impractical here, so this exercises
    /// the storage primitive directly: what must not regress is that
    /// `write().take()` is load-bearing and clone-and-read re-introduces
    /// the loop.
    #[test]
    fn take_consumes_last_word_so_repeated_fires_no_op() {
        let storage: Arc<RwLock<Option<LastWord>>> = Arc::new(RwLock::new(None));

        // Engine stashes a last word after auto-correcting `цщц`
        // → `wow `.
        *storage.write() = Some(LastWord {
            keys: Vec::new(),
            rendered: "цщц".into(),
            layout: LayoutId::new("uk-UA"),
            boundary_char: ' ',
            boundary_scancode: 0x39,
            boundary_shift: false,
            corrected_to: Some(LayoutId::new("en-US")),
        });

        // First fire of the manual hotkey: take wins, processes.
        let first = storage.write().take();
        assert!(
            first.is_some(),
            "first manual switch must see the stashed last_word"
        );

        // Echo / auto-repeat fires: subsequent takes find None.
        // This is what stops the loop and the sound spam.
        for _ in 0..50 {
            let echo = storage.write().take();
            assert!(
                echo.is_none(),
                "repeated manual-switch fires after the first must find None — \
                 if this regresses, the hotkey loop bug is back"
            );
        }
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
            timestamp_ms: 0,
        }
    }

    /// Regression: typing the Ukrainian word `Друже` while en-US is
    /// active produces the en-US render `Lhe;t` (because 0x27, the
    /// uk-UA letter `ж`, is `;` under en-US). The bare `;` made
    /// `looks_like_code_token` veto the auto-switch. The cleaned
    /// rendering should drop that `;` and read `Lhet`.
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

    /// Real `_` typed under en-US is genuine code intent — the
    /// scancode (0x0C with shift) is `_` in both layouts and not a
    /// letter anywhere. It must survive the cleanup so the
    /// snake_case heuristic still fires on real code.
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

    /// Fallback: if the current layout isn't in the DB the function
    /// should return the supplied `fallback` string untouched.
    #[test]
    fn falls_back_when_layout_missing() {
        let db = LayoutDb::load_embedded();
        let nonexistent = LayoutId::from("xx-YY");
        let cleaned = render_for_code_check(&[], &nonexistent, &db, "fallback");
        assert_eq!(cleaned, "fallback");
    }
}

mod layout_eligibility_tests {
    use super::is_layout_eligible;
    use poltertype_layout::LayoutId;

    fn id(s: &str) -> LayoutId {
        LayoutId::from(s)
    }

    /// The original "http " bug: detector picked `fr-FR` even though
    /// the user only had en-US / ru-RU / uk-UA active in the OS, and
    /// `switch_to(fr-FR)` then aborted *after* backspaces had already
    /// destroyed the word. The OS-active filter must drop fr-FR from
    /// the candidate set before the detector ever sees it.
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

    /// The current layout always passes, even if the OS list
    /// transiently doesn't report it. Without this, a query race could
    /// strip the layout the user is *currently typing in* from the
    /// candidate set, leaving the engine unable to render the buffer
    /// for the "keep current" code path.
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

    /// When the OS query fails (`None`) we fail open — fall back to the
    /// pre-fix behaviour where settings are the only filter. Better to
    /// occasionally pick an unreachable layout (caught by the
    /// apply_correction pre-flight) than freeze the engine entirely.
    #[test]
    fn fail_open_when_os_query_unavailable() {
        let current = id("uk-UA");
        assert!(is_layout_eligible(&id("fr-FR"), &current, &[], &[], None,));
    }

    /// Settings `ignored` always wins, even over OS-active. If a user
    /// disables a layout in our settings, we honour that regardless of
    /// what the OS reports.
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
        // First press fires.
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
        // Re-armed — a fresh press fires again.
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
        // The real chord still fires on its first press.
        assert!(match_chord(
            &ev(SPACE, KeyDirection::Press, ctrl_shift()),
            CTRL_SHIFT_SPACE,
            &mut down
        ));
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
        // Plain `v` is just a letter, not a paste.
        assert!(!is_paste_shortcut(&ev(
            SC_V,
            KeyDirection::Press,
            Modifiers::NONE
        )));
        // Ctrl+C must not be mistaken for a paste.
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
