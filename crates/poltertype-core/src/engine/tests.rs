//! Engine unit + integration tests, split out of `mod.rs`.
//!
//! This prelude re-imports the engine's public API plus the internal
//! submodules, so the inner test modules resolve names through
//! `use super::*` exactly as they did when they lived inline.

#![allow(unused_imports)]

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use parking_lot::{Mutex, RwLock};
use poltertype_detect::{Detector, Verdict};
use poltertype_input::{
    EmittedKey, FocusTracker, InputError, KeyDirection, KeyEmitter, KeyEvent, ReplayKey,
};
use poltertype_layout::{LayoutId, LayoutSwitcher};
use poltertype_types::SwitchAction;

use super::consts::*;
use super::heuristics::*;
use super::types::*;
use super::*;

/// Full-engine integration tests with mocked OS surfaces. These drive
/// `SwitcherEngine::run` on a real thread through the public channel
/// API — the same way `poltertype-app` does — and assert on the exact key
/// operations the engine emits. They are the regression net for the
/// two long-standing field reports:
///
/// * "після перемикання лишається перший символ старого слова" —
///   keystrokes racing the correction used to soak up backspaces;
/// * "видаляю пару символів, дописую — переводить пів слова" — the
///   buffer used to lose the word head across backspace-over-boundary
///   edits, so the next correction under-counted.
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
    }

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
            let mut settings = crate::settings::Settings::default();
            settings.engine.idle_timeout_ms = idle_timeout_ms;
            let settings = Arc::new(SettingsStore::for_tests(settings));
            let layouts = Arc::new(LayoutDb::load_embedded());
            let emitter = Arc::new(emitter);
            let mut switcher = MockSwitcher::new("en-US", &["en-US", "uk-UA"]);
            switcher.fail_switch = fail_switch;
            let switcher = Arc::new(switcher);
            let detectors: Vec<Box<dyn Detector>> = vec![Box::new(AlwaysOther(
                LayoutId::from("en-US"),
                LayoutId::from("uk-UA"),
            ))];
            let (key_tx, key_rx) = crossbeam_channel::bounded::<KeyEvent>(1024);
            let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded::<EngineCommand>();
            let (out_tx, out_rx) = crossbeam_channel::unbounded::<SwitcherEvent>();
            let engine = SwitcherEngine::new(
                Arc::clone(&settings),
                layouts,
                detectors,
                Arc::<MockSwitcher>::clone(&switcher) as Arc<dyn poltertype_layout::LayoutSwitcher>,
                Arc::<MockEmitter>::clone(&emitter) as Arc<dyn KeyEmitter>,
                Arc::new(NoopFocusTracker),
                Arc::new(crate::audio::AudioPlayer::new()),
                out_tx,
            );
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
            self.key_tx
                .send(KeyEvent {
                    vk: sc,
                    scancode: sc,
                    direction,
                    modifiers: poltertype_types::Modifiers {
                        shift,
                        ..poltertype_types::Modifiers::NONE
                    },
                    injected: false,
                    timestamp_ms: 0,
                })
                .expect("engine alive");
        }

        /// Wait until the engine has drained everything we sent AND
        /// its emit-op log has stopped moving. Corrections deliberately
        /// dawdle — quiet-gap absorption (~90 ms), the post-replay
        /// echo-settle wait (up to 400 ms when echoes never arrive, as
        /// with this mock), and chained decisions for absorbed words —
        /// so the stability window must outlast the engine's longest
        /// internal quiet stretch.
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

    /// The full fast-typing race: the user types the second word AND
    /// its boundary before the correction of the first word begins.
    /// Everything must come out in order — word1 corrected, boundary,
    /// word2 replayed behind it — and word2 must get its own decision
    /// (a second correction here, since the mock detector always
    /// flips).
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

    /// Regression for the manual-switch hotkey loop bug.
    ///
    /// The user types `цщц` (uk-UA), engine auto-corrects to `wow `,
    /// stashes `last_word`. User presses `Ctrl+Shift+Backspace` to
    /// re-apply manually. `apply_correction` sends BACKSPACE
    /// keystrokes via SendInput; those Backspaces are flagged
    /// INJECTED so the engine ignores them, but Win32
    /// `RegisterHotKey` (the primitive `global-hotkey` uses) sees
    /// the combination of our injected Backspace + the user's
    /// still-held Ctrl+Shift modifiers as another fresh
    /// `Ctrl+Shift+Backspace` press and fires the hotkey again.
    /// Same effect from key auto-repeat if the user holds the chord.
    ///
    /// Without atomic take-and-clear, every echo runs another
    /// `force_switch_last`, deleting + re-typing `wow ` and playing
    /// the correction sound. The user-visible symptom: text
    /// accumulates and a sound loop doesn't stop until the app is
    /// killed.
    ///
    /// The fix in `EngineCommand::SwitchLastForcefully` swaps from
    /// `read().clone()` to `write().take()`: the first fire
    /// processes; subsequent fires hit `None` and exit silently.
    /// To re-trigger, the user must complete another word and let
    /// the engine re-stash a new last_word.
    ///
    /// We can't easily construct a full `SwitcherEngine` here (lots
    /// of OS deps), so we exercise the storage primitive directly —
    /// what matters for the bug is that the take semantics are
    /// load-bearing, and a future refactor that switches them back
    /// to clone-and-read would re-introduce the loop. This test
    /// pins that.
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
