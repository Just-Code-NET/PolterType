//! `SwitcherEngine` itself — state, run loop, and the correction
//! machinery. Pure helpers live in [`super::heuristics`], plain
//! data types in [`super::types`].

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, select_biased};
use parking_lot::{Mutex, RwLock};
use poltertype_detect::{Detector, Verdict, looks_like_code_token};
use poltertype_input::{
    EmittedKey, FocusTracker, InputError, KeyDirection, KeyEmitter, KeyEvent, ReplayKey,
};
use poltertype_layout::{LayoutId, LayoutSwitcher};
use poltertype_types::SwitchAction;
use tracing::{debug, info, warn};

use crate::audio::{AudioPlayer, SoundEvent};
use crate::commands::{CommandAction, UserCommand, find_matching_command};
use crate::layouts::LayoutDb;
use crate::settings::SettingsStore;

use super::buffer::{KeyKind, WordBoundary, WordBuffer, classify};
use super::consts::PASTE_GUARD;
use super::enums::{Either, EngineCommand, SwitcherEvent};
use super::heuristics::{
    app_is_disabled, is_layout_eligible, is_paste_shortcut, is_structural_boundary,
    is_submission_boundary, is_submission_scancode, looks_like_all_caps, match_chord,
    render_for_code_check,
};
use super::types::{ChordState, KeystreamHotkeys, LastWord, WindowDrain};

pub struct SwitcherEngine {
    settings: Arc<SettingsStore>,
    layouts: Arc<LayoutDb>,
    detectors: Vec<Box<dyn Detector>>,
    layout_switcher: Arc<dyn LayoutSwitcher>,
    key_emitter: Arc<dyn KeyEmitter>,
    focus_tracker: Arc<dyn FocusTracker>,
    audio: Arc<AudioPlayer>,
    out_tx: Sender<SwitcherEvent>,
    paused: Arc<RwLock<bool>>,
    /// Buffer of the previous fully-completed word (for "switch-last").
    last_word: Arc<RwLock<Option<LastWord>>>,
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
    expected_echo: Mutex<VecDeque<(u32, Instant)>>,
    /// Hotkey chords matched directly off the key stream. Empty unless
    /// the app enables them (Wayland) via
    /// [`EngineCommand::SetKeystreamHotkeys`].
    keystream_hotkeys: RwLock<KeystreamHotkeys>,
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
    paste_guard_until: RwLock<Instant>,
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
            paste_guard_until: RwLock::new(Instant::now()),
        }
    }

    pub fn paused(&self) -> bool {
        *self.paused.read()
    }

    /// Drive the engine to completion. Returns when both channels close.
    pub fn run(self, key_rx: Receiver<KeyEvent>, cmd_rx: Receiver<EngineCommand>) {
        let mut buffer = WordBuffer::new();
        let mut last_event_at = Instant::now();
        let mut chord_state = ChordState::default();
        let idle_timeout = Duration::from_millis(self.settings.snapshot().engine.idle_timeout_ms);

        info!(
            detectors = ?self.detectors.iter().map(|d| d.name()).collect::<Vec<_>>(),
            layouts = self.layouts.len(),
            "engine running"
        );

        loop {
            // Block on whichever channel pings first; bias commands so
            // pause-toggle doesn't get starved by a torrent of keys.
            let event = select_biased! {
                recv(cmd_rx) -> msg => match msg {
                    Ok(cmd) => Either::Cmd(cmd),
                    Err(_) => break,
                },
                recv(key_rx) -> msg => match msg {
                    Ok(ev) => Either::Key(ev),
                    Err(_) => break,
                },
            };

            match event {
                Either::Cmd(cmd) => self.handle_command(cmd, &mut buffer, &key_rx),
                Either::Key(ev) => {
                    // Our own injected keystrokes echoing back (Linux
                    // behind keyd & friends) — swallow them before any
                    // other processing so they can't pollute the
                    // buffer, fire hotkey chords, or reset the idle
                    // clock semantics for real typing.
                    if self.consume_echo(&ev) {
                        last_event_at = Instant::now();
                        continue;
                    }
                    self.check_keystream_hotkeys(&ev, &mut chord_state, &mut buffer, &key_rx);
                    if last_event_at.elapsed() > idle_timeout {
                        debug!("idle timeout — abandoning word buffer");
                        // Not a plain clear: if a word was mid-flight
                        // the screen still holds its head, and
                        // correcting just the tail would chop the word
                        // in half. `abandon` taints the current word
                        // so the engine skips deciding on it.
                        buffer.abandon();
                        *self.last_word.write() = None;
                    }
                    last_event_at = Instant::now();
                    self.handle_key(ev, &mut buffer, &key_rx);

                    // After processing, drain non-blocking commands so
                    // hotkeys feel snappy even under heavy typing load.
                    while let Ok(cmd) = cmd_rx.try_recv() {
                        self.handle_command(cmd, &mut buffer, &key_rx);
                    }
                }
            }
        }

        info!("engine shutting down");
    }

    /// Match the raw key event against the keystream hotkeys (Wayland
    /// path). Mirrors what the OS `global-hotkey` grab does on other
    /// backends, dispatching the same [`EngineCommand`]s.
    ///
    /// Our own replayed corrections can't re-trigger a chord here:
    /// `injected` events are ignored outright, and untagged echoes
    /// (keyd & friends) were already consumed by `consume_echo` in the
    /// run loop before this is called — during a manual switch the
    /// user may still be holding `Ctrl+Shift` while our uinput
    /// backspaces echo back as `Ctrl+Shift+Backspace`. Run before
    /// the paused early-return in `handle_key` so the pause chord can
    /// also *resume*.
    fn check_keystream_hotkeys(
        &self,
        ev: &KeyEvent,
        state: &mut ChordState,
        buffer: &mut WordBuffer,
        key_rx: &Receiver<KeyEvent>,
    ) {
        if ev.injected {
            return;
        }
        let hk = *self.keystream_hotkeys.read();
        if let Some(c) = hk.pause {
            if match_chord(ev, c, &mut state.pause_key_down) {
                self.handle_command(EngineCommand::TogglePause, buffer, key_rx);
            }
        }
        if let Some(c) = hk.switch_last {
            if match_chord(ev, c, &mut state.switch_key_down) {
                self.handle_command(EngineCommand::SwitchLastForcefully, buffer, key_rx);
            }
        }
    }

    /// Record presses the emitter just put on the wire so their
    /// echoes can be consumed off the key stream.
    fn push_echoes(&self, emitted: Vec<EmittedKey>) {
        if emitted.is_empty() {
            return;
        }
        // keyd re-schedules our events through its own virtual
        // keyboard, so echoes can trail the emission — but not by
        // much. Keep the deadline tight: a stale entry that outlives
        // its echo would eat a real user press of the same scancode.
        // (`apply_correction` additionally waits out the queue right
        // after emitting, so entries rarely live past ~100 ms.)
        let deadline = Instant::now() + Duration::from_millis(800);
        let mut q = self.expected_echo.lock();
        q.extend(
            emitted
                .iter()
                .filter(|e| e.direction == KeyDirection::Press)
                .map(|e| (e.scancode, deadline)),
        );
        // Hygiene cap — a runaway queue must never eat minutes of
        // real typing.
        while q.len() > 256 {
            q.pop_front();
        }
    }

    /// True if `ev` is one of our own injected keystrokes echoing back
    /// through the listener (Linux behind an input remapper). Match-
    /// and-consume against the expected queue with a lookahead of one:
    /// remappers occasionally coalesce/drop one of our paced events,
    /// so if the head doesn't match but the entry behind it does, the
    /// head's echo is assumed lost and both entries are consumed.
    fn consume_echo(&self, ev: &KeyEvent) -> bool {
        if ev.direction != KeyDirection::Press {
            return false;
        }
        let mut q = self.expected_echo.lock();
        let now = Instant::now();
        while let Some(&(_, deadline)) = q.front() {
            if deadline < now {
                q.pop_front();
            } else {
                break;
            }
        }
        match q.front() {
            Some(&(sc, _)) if sc == ev.scancode => {
                q.pop_front();
                true
            }
            Some(_) => match q.get(1) {
                Some(&(sc1, _)) if sc1 == ev.scancode => {
                    q.pop_front();
                    q.pop_front();
                    true
                }
                _ => false,
            },
            None => false,
        }
    }

    fn handle_command(
        &self,
        cmd: EngineCommand,
        buffer: &mut WordBuffer,
        key_rx: &Receiver<KeyEvent>,
    ) {
        match cmd {
            EngineCommand::SetKeystreamHotkeys(hk) => {
                info!(?hk, "keystream hotkeys configured");
                *self.keystream_hotkeys.write() = hk;
            }
            EngineCommand::TogglePause => {
                let mut g = self.paused.write();
                *g = !*g;
                let now = *g;
                info!(paused = now, "pause toggled");
                let _ = self.out_tx.send(SwitcherEvent::PausedChanged(now));
                self.audio.play(if now {
                    SoundEvent::Pause
                } else {
                    SoundEvent::Resume
                });
            }
            EngineCommand::SwitchLastForcefully => {
                // Atomic take, NOT clone-and-read. Critical for the
                // hotkey-loop bug:
                //
                // The user types `цщц` (uk-UA), engine auto-corrects
                // to `wow ` and stashes last_word. User then presses
                // `Ctrl+Shift+Backspace` to manually re-apply. The
                // hotkey fires, we run `apply_correction`, which
                // sends BACKSPACE keystrokes via SendInput. Those
                // Backspaces are flagged INJECTED so the engine
                // ignores them — but the OS-level RegisterHotKey
                // (used by `global-hotkey`) sees the *combination*
                // of our injected Backspace + the user's still-held
                // Ctrl+Shift modifiers as a fresh `Ctrl+Shift+Backspace`
                // press, and fires the hotkey again. That ran
                // `force_switch_last` again, which sent another 4
                // backspaces, which fired the hotkey again — every
                // iteration both corrected the text again AND played
                // the correction sound, producing the user-visible
                // symptom: text accumulating to `wow wow wow…` and a
                // sound loop that didn't stop until the app was
                // killed.
                //
                // Auto-repeat on a held Backspace key would have the
                // same effect even without the modifier-combining
                // edge case.
                //
                // Taking + clearing last_word atomically means the
                // first fire processes; every subsequent fire from
                // the same physical hotkey press (or its echo) finds
                // `None` and exits silently. To re-trigger, the user
                // must complete another word and let the engine
                // re-stash a new last_word.
                let taken = self.last_word.write().take();
                if let Some(last) = taken {
                    self.force_switch_last(last, buffer, key_rx);
                } else {
                    debug!(
                        "manual switch-last fired but no last word stashed (likely a duplicate from key auto-repeat)"
                    );
                }
            }
            EngineCommand::SettingsReloaded => {
                self.audio.refresh_from(&self.settings);
                buffer.reset();
            }
        }
    }

    fn handle_key(&self, ev: KeyEvent, buffer: &mut WordBuffer, key_rx: &Receiver<KeyEvent>) {
        if ev.injected {
            // Avoid feedback: our own corrections come back through here
            // (Windows / macOS tag them; Linux echoes were consumed by
            // `consume_echo` in the run loop).
            return;
        }
        if *self.paused.read() {
            return;
        }
        // A paste shortcut opens a window during which we won't
        // auto-correct: the pasted text isn't something the user typed,
        // and on Wayland it can echo back to us as synthetic keystrokes.
        // See `paste_guard_until`.
        if is_paste_shortcut(&ev) {
            *self.paste_guard_until.write() = Instant::now() + PASTE_GUARD;
        }
        if ev.modifiers.is_command() {
            // Shortcuts (Ctrl+C, Cmd+V, …) — abandon, don't accumulate.
            // A shortcut can edit text arbitrarily (Ctrl+X, Ctrl+Z), so
            // a word that was mid-flight is no longer trustworthy;
            // `abandon` taints it. The stashed last-word survives —
            // the manual switch-last chord itself is a shortcut.
            buffer.abandon();
            return;
        }

        // Cross-layout letter hint: keeps Cyrillic words intact when
        // typed under en-US (`б` at 0x33 would otherwise look like a
        // `,` boundary). See `WordBuffer::feed` for the full rationale.
        // Shift-aware so adding more layouts (de-DE / fr-FR / …) doesn't
        // accidentally classify genuine en-US punctuation as "letter
        // in another layout".
        let letter_in_any_layout = self
            .layouts
            .is_letter_in_any_layout(ev.scancode, ev.modifiers.shift);
        // Resolve the character this scancode produces under the
        // *currently active* layout — the buffer needs that to
        // classify (a `,`-position scancode is `б` in uk-UA, etc.).
        // Only when the classification actually depends on it: the
        // cross-layout hint alone settles letters, and releases never
        // reach the classifier.
        let produced = if ev.direction == KeyDirection::Press && !letter_in_any_layout {
            self.translate_via_current_layout(ev.scancode, ev.modifiers.shift)
        } else {
            None
        };

        match buffer.feed(ev, produced, letter_in_any_layout) {
            WordBoundary::InProgress => {}
            WordBoundary::Abandoned => {
                // Caret went somewhere unknown (click / nav / Esc) —
                // a stashed last-word would be corrected at the wrong
                // screen position now.
                *self.last_word.write() = None;
            }
            WordBoundary::WordCompleted {
                boundary_scancode,
                boundary_shift,
                tainted,
            } => {
                if tainted {
                    debug!("completed word is tainted — skipping decision");
                    *self.last_word.write() = None;
                    let _ = self.out_tx.send(SwitcherEvent::KeptCurrent {
                        reason: "buffer lost track of this word (caret moved / idle / edited \
                                 past word start) — not correcting"
                            .into(),
                    });
                } else if Instant::now() < *self.paste_guard_until.read() {
                    // Word completed inside the post-paste window —
                    // almost certainly pasted text replayed as
                    // keystrokes, not typing. Drop it without
                    // correcting.
                    debug!("paste guard active — skipping correction for completed word");
                } else {
                    self.decide(buffer, boundary_scancode, boundary_shift, key_rx);
                }
            }
        }
    }

    /// Best-effort current-layout translate. Returns `None` if we
    /// can't query the OS or the scancode isn't in the mapping
    /// table — both are normal for control / OEM keys.
    fn translate_via_current_layout(&self, scancode: u32, shift: bool) -> Option<char> {
        let current = self.layout_switcher.current().ok()?;
        let mapping = self.layouts.get(&current)?;
        mapping.translate_key(poltertype_types::WordKey {
            scancode,
            shift,
            timestamp_ms: 0,
        })
    }

    fn decide(
        &self,
        buffer: &mut WordBuffer,
        boundary_scancode: u32,
        boundary_shift: bool,
        key_rx: &Receiver<KeyEvent>,
    ) {
        let snap = self.settings.snapshot();
        let keys = buffer.completed().to_vec();
        if keys.is_empty() {
            return;
        }
        // Note: no global min_word_length gate here. Each detector
        // decides on its own — the dictionary detector wants to see
        // single-letter prepositions; word-plausibility self-filters
        // to ≥3 letters.

        let current_layout = match self.layout_switcher.current() {
            Ok(l) => l,
            Err(e) => {
                warn!(?e, "could not query current layout; skipping decision");
                return;
            }
        };

        // Filter the candidate set down to layouts the engine can
        // actually switch to. Three layers, all must pass:
        //
        //   1. `[languages].active` from settings — empty means "every
        //      loaded layout".
        //   2. `[languages].ignored` from settings — always vetoes.
        //   3. The OS's current active-layouts list — anything not
        //      installed/enabled in the OS is unreachable. Including it
        //      here previously caused the detector to pick e.g. `fr-FR`
        //      for `http` even when the user only has en-US / ru-RU /
        //      uk-UA installed — `switch_to()` would then reject it,
        //      and because backspaces had already been emitted the word
        //      was simply destroyed. Filtering here means the detector
        //      never sees the unreachable layout to begin with.
        //
        // If the OS query fails we fail-open (skip layer 3), matching
        // the previous behaviour. The pre-flight check inside
        // `apply_correction` is the second line of defence.
        let os_active: Option<Vec<LayoutId>> = match self.layout_switcher.list_active() {
            Ok(list) => Some(list),
            Err(e) => {
                warn!(
                    ?e,
                    "could not list active OS layouts; skipping OS-active filter"
                );
                None
            }
        };
        let active: &[LayoutId] = &snap.languages.active;
        let ignored: &[LayoutId] = &snap.languages.ignored;
        let candidates: Vec<(LayoutId, String)> = self
            .layouts
            .iter()
            .filter(|(id, _)| {
                is_layout_eligible(id, &current_layout, active, ignored, os_active.as_deref())
            })
            .map(|(id, m)| (id.clone(), m.translate_buffer(&keys)))
            .collect();

        // ---- Stash the rendered current text early; "force switch
        // last" uses it whether or not auto-decision proceeds. ----
        let current_text = candidates
            .iter()
            .find(|(l, _)| l == &current_layout)
            .map(|(_, t)| t.clone())
            .unwrap_or_default();

        // Resolve boundary scancode → character (even if we skip the
        // decision below, store it so the manual hotkey works).
        let boundary_char = self
            .layouts
            .get(&current_layout)
            .and_then(|m| {
                m.translate_key(poltertype_types::WordKey {
                    scancode: boundary_scancode,
                    shift: boundary_shift,
                    timestamp_ms: 0,
                })
            })
            .or(match boundary_scancode {
                0x39 => Some(' '),
                0x1C | 0x60 => Some('\n'), // Enter / numpad Enter
                0x0F => Some('\t'),
                _ => None,
            })
            .unwrap_or(' ');

        *self.last_word.write() = Some(LastWord {
            keys: keys.clone(),
            rendered: current_text.clone(),
            layout: current_layout.clone(),
            boundary_char,
            boundary_scancode,
            boundary_shift,
        });

        // ---- Smart commands (text triggers) ----
        //
        // Consult the user's `[[commands]]` list BEFORE the auto-
        // switch filters: text expansion is a direct user
        // intent ("I typed `anrl ` because I want it expanded"),
        // not a guess the engine is making, so the structural-
        // boundary / disabled-app / identifier filters don't apply.
        //
        // The match is on the rendering in the CURRENT layout —
        // typing `anrl ` under en-US matches `anrl`, but typing the
        // same physical keys under uk-UA produces a Cyrillic
        // rendering that won't match (which is the right behaviour:
        // an English acronym typed accidentally in Ukrainian layout
        // should go through normal layout-correction, not
        // expansion).
        let focused_basename = self.focus_tracker.focused_exe().and_then(|exe| {
            std::path::Path::new(&exe)
                .file_name()
                .and_then(|f| f.to_str())
                .map(str::to_owned)
        });
        if let Some(cmd) =
            find_matching_command(&snap.commands, &current_text, focused_basename.as_deref())
        {
            // Erase one on-screen character per buffered key plus the
            // boundary. Counting keys (not rendered chars) survives
            // scancodes our mapping table can't render — the screen
            // still shows a character for those.
            self.dispatch_smart_command(cmd, keys.len() + 1, boundary_char);
            // The trigger text no longer exists on screen — the word
            // must not be re-openable via backspace.
            buffer.forget_completed();
            return;
        }

        // ---- Pre-decision filters (auto-switch only) ----
        //
        // Filters apply *only* to automatic decisions — the manual
        // switch-last hotkey calls force_switch_last directly and
        // bypasses all of them. That's the dev-friendly contract:
        // we stay quiet by default in code / URL / path contexts,
        // but if the user explicitly hits the hotkey we always do
        // the switch.

        // Filter 0a: submission / navigation boundary (Enter, Tab).
        // Re-emitting one of these as part of the correction replay
        // executes a shell command, sends a chat message, or fires a
        // completion — and the correction fires too late to be useful
        // anyway (the line is already submitted). Stay out of it.
        if is_submission_boundary(boundary_char) {
            debug!(
                token = %current_text,
                "skipping auto-switch: submission boundary (Enter/Tab)"
            );
            let _ = self.out_tx.send(SwitcherEvent::KeptCurrent {
                reason: format!(
                    "submission boundary after `{current_text}` — not re-emitting Enter/Tab"
                ),
            });
            return;
        }

        // Filter 0: structural boundary character. If the user
        // ended the word with `:` / `/` / `\` / `@` / `=` / `#` /
        // `&` then they're typing a URL / path / email / config
        // expression / code, NOT prose. Switching `http` to `реез`
        // because they just typed `:` would corrupt the URL they're
        // half-way through. Skip.
        if is_structural_boundary(boundary_char) {
            debug!(
                token = %current_text,
                boundary = %boundary_char,
                "skipping auto-switch: structural boundary"
            );
            let _ = self.out_tx.send(SwitcherEvent::KeptCurrent {
                reason: format!(
                    "structural boundary `{boundary_char}` after `{current_text}` — likely URL / path / email / code"
                ),
            });
            return;
        }

        // Filter 0c: ALL-CAPS word (held Shift / Caps Lock through
        // the whole token). The user typing `URL`, `HTTP`, `API`,
        // `ССЫЛКА` is being deliberate — they're not "in the wrong
        // layout", they're spelling out an abbreviation. Auto-
        // switching here is the classic glitchy case: the all-caps
        // token often happens to render as something letter-like in
        // the other layout, the detector takes the bait, and the
        // user watches their abbreviation get replaced with
        // gibberish.
        //
        // Held-Shift catches the case on every backend. Caps Lock
        // catches it on Linux/Wayland (the listener XORs caps into
        // the shift bit before the engine sees the event). Windows /
        // macOS don't yet fold Caps Lock into the modifier — a
        // separate fix at the per-OS listener level — but the
        // held-Shift variant covers most ALL-CAPS typing there too.
        //
        // The manual switch-last hotkey still works on these buffers:
        // `last_word` was stashed above before any filter ran.
        if snap.engine.suppress_for_all_caps && looks_like_all_caps(&current_text) {
            debug!(
                token = %current_text,
                "skipping auto-switch: word is ALL CAPS (likely abbreviation)"
            );
            let _ = self.out_tx.send(SwitcherEvent::KeptCurrent {
                reason: format!(
                    "`{current_text}` is ALL CAPS — likely an abbreviation, not a wrong-layout word"
                ),
            });
            return;
        }

        // Filter 1: focused app on the disabled list.
        if let Some(exe) = self.focus_tracker.focused_exe() {
            if app_is_disabled(&exe, &snap.exceptions.disabled_apps) {
                debug!(%exe, "skipping auto-switch: app on disabled_apps list");
                let _ = self.out_tx.send(SwitcherEvent::KeptCurrent {
                    reason: format!("app `{exe}` on disabled_apps list"),
                });
                return;
            }
        }

        // Filter 2: token looks like an identifier (camelCase /
        // snake_case / letter+digit / code punct).
        //
        // We feed `looks_like_code_token` a *cleaned* rendering that
        // strips cross-layout artifacts — characters whose scancode is
        // a letter in some other layout but renders as punctuation
        // under the current one. Without this, typing a Ukrainian word
        // containing `ж` (scancode 0x27) under en-US produces a `;`
        // mid-string and the heuristic would (wrongly) call the buffer
        // "code". See `render_for_code_check` for details.
        let token_for_code_check =
            render_for_code_check(&keys, &current_layout, &self.layouts, &current_text);
        if snap.engine.suppress_in_identifiers && looks_like_code_token(&token_for_code_check) {
            debug!(
                token = %current_text,
                cleaned = %token_for_code_check,
                "skipping auto-switch: looks like code identifier"
            );
            let _ = self.out_tx.send(SwitcherEvent::KeptCurrent {
                reason: format!("token `{current_text}` looks like an identifier"),
            });
            return;
        }

        let ctx = poltertype_detect::DetectionContext {
            current_layout: &current_layout,
            candidates: &candidates,
            recent_context: "",
        };

        // Run detectors in priority order. The first non-NoOpinion
        // verdict wins — including a `Keep` veto, which short-circuits
        // the rest of the pipeline.
        let mut chosen: Option<Verdict> = None;
        for d in &self.detectors {
            match d.judge(&ctx) {
                Verdict::NoOpinion => continue,
                v => {
                    chosen = Some(v);
                    break;
                }
            }
        }

        let action = match chosen {
            Some(Verdict::Keep { reason }) => SwitchAction::KeepCurrent {
                reason: format!("veto by detector: {reason}"),
            },
            Some(Verdict::Switch(v)) if v.confidence >= snap.engine.confidence_threshold => {
                let target_text = candidates
                    .iter()
                    .find(|(l, _)| l == &v.best_layout)
                    .map(|(_, t)| t.clone())
                    .unwrap_or_default();
                // The boundary key has already been delivered to the
                // focused app, so we have to delete it too and re-emit
                // a copy after the corrected word.
                let mut corrected_with_boundary = target_text;
                corrected_with_boundary.push(boundary_char);
                SwitchAction::SwitchAndReplay {
                    target_layout: v.best_layout,
                    corrected_text: corrected_with_boundary,
                    // One on-screen character per buffered key + the
                    // boundary. Keys (not rendered chars): a scancode
                    // missing from our mapping table still produced a
                    // character on screen, and under-counting here is
                    // exactly how word heads get left behind.
                    backspaces: keys.len() + 1,
                    reason: v.reason,
                }
            }
            Some(Verdict::Switch(v)) => SwitchAction::KeepCurrent {
                reason: format!(
                    "detector confidence {:.2} below threshold {:.2}",
                    v.confidence, snap.engine.confidence_threshold
                ),
            },
            Some(Verdict::NoOpinion) | None => SwitchAction::KeepCurrent {
                reason: "no detector had an opinion".into(),
            },
        };

        match action {
            SwitchAction::KeepCurrent { reason } => {
                debug!(%reason, "decision: keep current");
                let _ = self.out_tx.send(SwitcherEvent::KeptCurrent { reason });
            }
            SwitchAction::SwitchAndReplay {
                target_layout,
                corrected_text,
                backspaces,
                reason,
            } => {
                // Build the replay sequence: original word scancodes
                // + the boundary key. After the layout flips, emitting
                // these same scancodes against the new xkb mapping
                // produces the corrected glyphs — no Unicode-compose
                // dance needed on Wayland.
                let mut replay: Vec<ReplayKey> = keys
                    .iter()
                    .map(|k| ReplayKey {
                        scancode: k.scancode,
                        shift: k.shift,
                    })
                    .collect();
                replay.push(ReplayKey {
                    scancode: boundary_scancode,
                    shift: boundary_shift,
                });
                self.apply_correction(
                    &current_layout,
                    &target_layout,
                    &current_text,
                    &corrected_text,
                    backspaces,
                    &reason,
                    snap.general.sound_on_correct,
                    Some(&replay),
                    Some((key_rx, buffer)),
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_correction(
        &self,
        from: &LayoutId,
        to: &LayoutId,
        original: &str,
        corrected: &str,
        backspaces: usize,
        reason: &str,
        play_sound: bool,
        replay_keys: Option<&[ReplayKey]>,
        live: Option<(&Receiver<KeyEvent>, &mut WordBuffer)>,
    ) {
        debug!(%from, %to, %original, %corrected, %reason, "applying correction");

        // Pre-flight: confirm the target layout is currently active in
        // the OS BEFORE we touch the user's text.
        //
        // The candidate filter inside `decide()` should already make
        // this impossible for auto-decisions, but keeping the check
        // here closes two more holes:
        //   * `force_switch_last` (manual hotkey) bypasses the
        //     candidate filter and can target any layout in `LayoutDb`.
        //   * Race: settings reload / OS layout list change between
        //     `decide()` and the actual key emission.
        //
        // On query failure we fall through and let `switch_to` surface
        // the original error (still safe — no keystrokes sent yet).
        match self.layout_switcher.list_active() {
            Ok(list) if !list.contains(to) => {
                warn!(
                    target = %to,
                    active = ?list,
                    "target layout not active in OS; aborting correction before any keystrokes"
                );
                return;
            }
            Err(e) => {
                warn!(
                    ?e,
                    "could not list active layouts before correction; continuing"
                );
            }
            _ => {} // active list contains target — proceed.
        }

        // Switch the layout FIRST. Flipping the layout doesn't touch
        // existing text, so if it fails we abort with the user's word
        // fully intact (the old order — backspaces, then switch —
        // destroyed the word whenever the switch failed). It also
        // overlaps the compositor's xkb propagation with the backspace
        // burst, and means any keystrokes the user lands mid-correction
        // already produce glyphs in the layout they intended.
        if let Err(e) = self.layout_switcher.switch_to(to) {
            warn!(?e, target = %to, "layout switch failed; aborting correction before any keystrokes");
            return;
        }

        // ── Absorb: wait for the user's fingers to lift ─────────────
        //
        // Keystrokes the user lands while our backspaces / replay are
        // on the wire physically interleave with them at the
        // compositor — the result is a scrambled word (`рприивіт`)
        // that no amount of after-the-fact counting can fix. So
        // before deleting anything, watch the key stream: as long as
        // presses keep arriving, keep absorbing them into the plan
        // (they are the start of the user's next word — on screen
        // already, in the layout we just switched to). Only when the
        // stream has been quiet for two probes (~50 ms — within a
        // fast typist's inter-key gap) do we start emitting. The
        // absorbed tail is deleted together with the word and
        // re-typed after the boundary, preserving order.
        //
        // If a *boundary* arrives while absorbing, the user finished
        // their next word too — stop there, include it, and re-process
        // it after the correction so the next word gets its own
        // decision. If Enter/Tab (submission) or anything murkier
        // (Backspace, nav, shortcut) arrives, abort the whole
        // correction — nothing has been emitted yet, so the user's
        // text is untouched; we just leave the layout switched and
        // taint the buffer.
        let mut live = live;
        let mut tail: Vec<KeyEvent> = Vec::new();
        let mut resume: Option<KeyEvent> = None;
        let mut suspicious = false;
        if let Some((rx, _)) = live.as_ref() {
            let deadline = Instant::now() + Duration::from_millis(600);
            let mut quiet_probes = 0u8;
            loop {
                let w = self.drain_correction_window(rx);
                tail.extend(w.word_keys);
                suspicious |= w.suspicious;
                if let Some(r) = w.resume {
                    if is_submission_scancode(r.scancode) {
                        suspicious = true;
                    } else {
                        resume = Some(r);
                    }
                    break;
                }
                if suspicious {
                    break;
                }
                if w.saw_user_press {
                    quiet_probes = 0;
                } else {
                    quiet_probes += 1;
                    // ~90 ms of silence. A fast typist's inter-key gap
                    // is ~45-60 ms plus listener lag, so two probes
                    // can land inside a single gap — three cannot.
                    if quiet_probes >= 3 {
                        break;
                    }
                }
                if Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(Duration::from_millis(30));
            }
        }

        if suspicious {
            // Nothing emitted yet — bail out with the text untouched.
            // The buffer can't vouch for the screen any more, though:
            // taint it and drop the manual-switch stash.
            debug!("uncertain keystrokes while preparing correction — aborting untouched");
            if let Some((_, buffer)) = live.as_mut() {
                self.seed_buffer(&tail, buffer);
                buffer.poison();
            }
            *self.last_word.write() = None;
            let _ = self.out_tx.send(SwitcherEvent::LayoutChanged(to.clone()));
            return;
        }

        // ── Delete: word + boundary + absorbed tail (+ its boundary) ─
        //
        // A bounded compensation loop catches the stragglers that
        // still manage to land during the burst itself: each one both
        // soaked up one of our backspaces and must be deleted and
        // re-typed, so it costs exactly one extra backspace either way.
        let mut to_delete = backspaces + tail.len() + usize::from(resume.is_some());
        for round in 0..3 {
            let sent = self.key_emitter.send_backspaces(to_delete);
            self.push_echoes(self.key_emitter.take_emitted());
            if let Err(e) = sent {
                warn!(?e, "send_backspaces failed; aborting correction");
                return;
            }
            let Some((rx, _)) = live.as_ref() else { break };
            // Give raced physical events a moment to travel
            // device → listener thread → our channel.
            std::thread::sleep(Duration::from_millis(12));
            let w = self.drain_correction_window(rx);
            suspicious |= w.suspicious;
            let mut extra = w.word_keys.len();
            tail.extend(w.word_keys);
            if let Some(r) = w.resume {
                if is_submission_scancode(r.scancode) || resume.is_some() {
                    // A second boundary (or a submission key) landed
                    // mid-deletion — too murky to reconstruct.
                    suspicious = true;
                } else {
                    resume = Some(r);
                    extra += 1;
                }
            }
            if extra == 0 {
                break;
            }
            debug!(
                extra,
                round, "user keystrokes raced the deletion; compensating"
            );
            to_delete = extra;
        }

        // ── Replay: word + boundary + tail (+ resume boundary) ──────
        //
        // Prefer replaying the original scancodes against the freshly
        // switched layout (the only path that works in Wayland-native
        // / terminal apps). Backends that have a real Unicode-emit API
        // (`KEYEVENTF_UNICODE`, `CGEventKeyboardSetUnicodeString`)
        // return `Unsupported`; we fall back to `send_text` for them.
        let extra_keys: Vec<ReplayKey> = tail
            .iter()
            .chain(resume.iter())
            .map(|ev| ReplayKey {
                scancode: ev.scancode,
                shift: ev.modifiers.shift,
            })
            .collect();
        let replayed = match replay_keys {
            Some(rk) => {
                let mut full: Vec<ReplayKey> = rk.to_vec();
                full.extend(extra_keys.iter().copied());
                let sent = self.key_emitter.send_keys(&full);
                self.push_echoes(self.key_emitter.take_emitted());
                match sent {
                    Ok(()) => true,
                    Err(InputError::Unsupported(_)) => false,
                    Err(e) => {
                        warn!(?e, "send_keys failed; correction may be partial");
                        return;
                    }
                }
            }
            None => false,
        };
        if !replayed {
            let mut text = corrected.to_owned();
            if let Some(mapping) = self.layouts.get(to) {
                for k in &extra_keys {
                    if let Some(c) = mapping.translate_key(poltertype_types::WordKey {
                        scancode: k.scancode,
                        shift: k.shift,
                        timestamp_ms: 0,
                    }) {
                        text.push(c);
                    }
                }
            }
            let sent = self.key_emitter.send_text(&text);
            self.push_echoes(self.key_emitter.take_emitted());
            if let Err(e) = sent {
                warn!(?e, "send_text failed; correction may be partial");
                return;
            }
        }

        if play_sound {
            self.audio.play(SoundEvent::Correct);
        }
        let _ = self.out_tx.send(SwitcherEvent::Corrected {
            from_layout: from.clone(),
            to_layout: to.clone(),
            original_text: original.to_owned(),
            corrected_text: corrected.to_owned(),
            reason: reason.to_owned(),
        });
        let _ = self.out_tx.send(SwitcherEvent::LayoutChanged(to.clone()));

        // ── Settle & seed ───────────────────────────────────────────
        if let Some((rx, buffer)) = live {
            // Wait out our own echoes before letting the run loop
            // resume. Rationale: consume_echo matches by scancode, so
            // while the queue is non-empty a REAL user press of the
            // same scancode we just replayed would be swallowed as an
            // echo (very possible right after a correction — the next
            // word often starts with the same letters). Draining here,
            // while the user is still inside the pause the absorb gate
            // verified, empties the queue in one keyd round-trip;
            // anything the user types afterwards can't be mistaken for
            // an echo. Bounded: backends whose echoes never come back
            // through the listener (Windows / macOS tag them injected
            // instead) just wait out the deadline once — after
            // emission, so the user never sees the latency.
            let mut post_tail: Vec<KeyEvent> = Vec::new();
            let mut post_resume: Option<KeyEvent> = None;
            let settle_deadline = Instant::now() + Duration::from_millis(400);
            loop {
                let w = self.drain_correction_window(rx);
                post_tail.extend(w.word_keys);
                suspicious |= w.suspicious;
                if let Some(r) = w.resume {
                    if post_resume.is_some() || is_submission_scancode(r.scancode) {
                        suspicious = true;
                    } else {
                        post_resume = Some(r);
                    }
                }
                if !self.echo_pending() || Instant::now() >= settle_deadline {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }

            if suspicious {
                // Something we couldn't attribute cleanly landed
                // mid-correction. The screen state is uncertain until
                // the next boundary — track nothing, correct nothing.
                buffer.abandon();
                buffer.poison();
                *self.last_word.write() = None;
            } else {
                // Chronological re-assembly of everything the user
                // typed while we were busy: the absorbed tail (word
                // in progress), its boundary (routed through the
                // normal pipeline so that word gets its own decision
                // — usually "keep", it was typed post-switch), then
                // whatever arrived after the replay.
                self.seed_buffer(&tail, buffer);
                if let Some(r) = resume {
                    self.handle_key(r, buffer, rx);
                }
                self.seed_buffer(&post_tail, buffer);
                if let Some(r) = post_resume {
                    self.handle_key(r, buffer, rx);
                }
            }
        }
    }

    fn echo_pending(&self) -> bool {
        !self.expected_echo.lock().is_empty()
    }

    /// Feed absorbed keystrokes into the buffer as the in-progress
    /// word (they are on screen after the corrected boundary).
    fn seed_buffer(&self, tail: &[KeyEvent], buffer: &mut WordBuffer) {
        for ev in tail {
            let letter = self
                .layouts
                .is_letter_in_any_layout(ev.scancode, ev.modifiers.shift);
            let produced = if letter {
                None
            } else {
                self.translate_via_current_layout(ev.scancode, ev.modifiers.shift)
            };
            let _ = buffer.feed(*ev, produced, letter);
        }
    }

    /// Drain everything currently pending on the listener channel,
    /// swallowing our own echoes. Collects the plain word-key presses
    /// the user managed to type while a correction was in flight;
    /// stops at the first boundary press (`resume` — the user finished
    /// their next word too). Anything murkier (Backspace, nav, click,
    /// shortcut) sets `suspicious`.
    fn drain_correction_window(&self, rx: &Receiver<KeyEvent>) -> WindowDrain {
        let mut out = WindowDrain::default();
        while let Ok(ev) = rx.try_recv() {
            if self.consume_echo(&ev) {
                continue;
            }
            if ev.injected || ev.direction != KeyDirection::Press {
                continue;
            }
            out.saw_user_press = true;
            if is_paste_shortcut(&ev) {
                *self.paste_guard_until.write() = Instant::now() + PASTE_GUARD;
            }
            if ev.modifiers.is_command() {
                out.suspicious = true;
                break;
            }
            let letter = self
                .layouts
                .is_letter_in_any_layout(ev.scancode, ev.modifiers.shift);
            let produced = if letter {
                None
            } else {
                self.translate_via_current_layout(ev.scancode, ev.modifiers.shift)
            };
            match classify(ev.scancode, produced, letter) {
                KeyKind::Word => out.word_keys.push(ev),
                KeyKind::Discard => {}
                KeyKind::Boundary => {
                    out.resume = Some(ev);
                    break;
                }
                // Backspace / nav / click mid-correction — can't
                // reconstruct where it landed.
                KeyKind::Backspace | KeyKind::EndAndDiscard => {
                    out.suspicious = true;
                    break;
                }
            }
        }
        out
    }

    /// Run a matched smart command. The shape is the same for every
    /// action variant: backspace the typed trigger plus the
    /// boundary character (so the magic word disappears from the
    /// user's text), then dispatch the action.
    ///
    /// `backspace_count` is precomputed by the caller as
    /// `current_text.chars().count() + 1` — counting characters not
    /// bytes, because that's what the OS-level emit_backspace uses
    /// across Cyrillic / multibyte triggers.
    ///
    /// For `TypeText` we re-emit the boundary character after the
    /// expansion so the user's flow continues naturally — typing
    /// `anrl<space>` ends up with the cursor after a trailing space
    /// in the expansion, not glued to the last word. For
    /// `SwitchLayout` and `OpenPath` the boundary stays consumed
    /// (the user wanted a side-effect, not text).
    fn dispatch_smart_command(
        &self,
        cmd: &UserCommand,
        backspace_count: usize,
        boundary_char: char,
    ) {
        info!(
            id = %cmd.id,
            trigger = %cmd.trigger,
            action = ?cmd.action,
            "smart command fired"
        );
        let sent = self.key_emitter.send_backspaces(backspace_count);
        self.push_echoes(self.key_emitter.take_emitted());
        if let Err(e) = sent {
            warn!(?e, id = %cmd.id, "smart command: send_backspaces failed");
            return;
        }
        match &cmd.action {
            CommandAction::TypeText { text } => {
                let sent = self.key_emitter.send_text(text);
                self.push_echoes(self.key_emitter.take_emitted());
                if let Err(e) = sent {
                    warn!(?e, id = %cmd.id, "smart command: send_text failed");
                    return;
                }
                // Re-emit the boundary so the user's typing flow
                // continues — they typed `anrl<space>`, they expect
                // `<expansion><space>` afterward, not the cursor
                // glued to the end.
                let mut buf = [0u8; 4];
                let s = boundary_char.encode_utf8(&mut buf);
                let sent = self.key_emitter.send_text(s);
                self.push_echoes(self.key_emitter.take_emitted());
                if let Err(e) = sent {
                    warn!(?e, id = %cmd.id, "smart command: re-emit boundary failed");
                }
            }
            CommandAction::SwitchLayout { layout } => {
                // Same pre-flight as `apply_correction`: confirm the
                // layout is reachable before doing anything else.
                // Backspaces have already happened — if the switch
                // is impossible we can't recover the deleted text,
                // but we log loudly so the user notices.
                match self.layout_switcher.list_active() {
                    Ok(list) if !list.contains(layout) => {
                        warn!(
                            target = %layout,
                            active = ?list,
                            id = %cmd.id,
                            "smart command: target layout not active in OS"
                        );
                        return;
                    }
                    Err(e) => {
                        warn!(
                            ?e,
                            id = %cmd.id,
                            "could not verify target layout availability; trying anyway"
                        );
                    }
                    _ => {}
                }
                if let Err(e) = self.layout_switcher.switch_to(layout) {
                    warn!(?e, id = %cmd.id, target = %layout, "smart command: switch failed");
                }
            }
            CommandAction::OpenPath { path } => {
                if let Err(e) = opener::open(path) {
                    warn!(?e, id = %cmd.id, path = %path, "smart command: open failed");
                }
            }
        }
    }

    fn force_switch_last(
        &self,
        last: LastWord,
        buffer: &mut WordBuffer,
        key_rx: &Receiver<KeyEvent>,
    ) {
        // Pick the most plausible alternate layout — in v0.1 with two
        // layouts, "the other one" is fine. Generalisation will
        // re-run the detector pipeline with `min_advantage = 0`.
        let other = self.layouts.ids().find(|id| **id != last.layout).cloned();
        let Some(target) = other else {
            warn!("only one layout known; can't force-switch");
            return;
        };
        let target_mapping = match self.layouts.get(&target) {
            Some(m) => m,
            None => {
                warn!(%target, "target layout not in DB");
                return;
            }
        };
        let mut corrected = target_mapping.translate_buffer(&last.keys);
        corrected.push(last.boundary_char);
        // Replay the boundary the user actually typed — except
        // Enter/Tab, where a re-press would submit the line / move
        // focus; substitute a space for those.
        let (boundary_sc, boundary_shift) = match last.boundary_scancode {
            0x1C | 0x0F | 0x60 => (0x39, false),
            sc => (sc, last.boundary_shift),
        };
        let mut replay: Vec<ReplayKey> = last
            .keys
            .iter()
            .map(|k| ReplayKey {
                scancode: k.scancode,
                shift: k.shift,
            })
            .collect();
        replay.push(ReplayKey {
            scancode: boundary_sc,
            shift: boundary_shift,
        });
        self.apply_correction(
            &last.layout,
            &target,
            &last.rendered,
            &corrected,
            last.keys.len() + 1,
            "manual switch-last hotkey",
            true,
            Some(&replay),
            Some((key_rx, buffer)),
        );
    }
}
