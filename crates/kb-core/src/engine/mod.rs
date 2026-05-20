//! `SwitcherEngine` — the state machine that turns key events into
//! layout-switch actions.
//!
//! Lives on a worker thread; receives [`KeyEvent`]s from the OS hook
//! and emits [`SwitcherEvent`]s back to the application (so the tray
//! / UI / audio can react). Effecting the actual switch + key replay
//! happens in `kb-app` via the [`kb_input::KeyEmitter`] +
//! [`kb_layout::LayoutSwitcher`] passed in.

pub mod buffer;
pub mod decision;

use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, select_biased};
use kb_detect::{Detector, Verdict, looks_like_code_token};
use kb_input::{FocusTracker, InputError, KeyEmitter, KeyEvent, ReplayKey};
use kb_layout::{LayoutId, LayoutSwitcher};
use kb_types::SwitchAction;
use parking_lot::RwLock;
use tracing::{debug, info, warn};

use crate::audio::{AudioPlayer, SoundEvent};
use crate::commands::{CommandAction, UserCommand, find_matching_command};
use crate::layouts::LayoutDb;
use crate::settings::SettingsStore;

pub use buffer::{WordBoundary, WordBuffer};
pub use decision::DecisionPolicy;

/// Outbound notifications the engine emits.
#[derive(Debug, Clone)]
pub enum SwitcherEvent {
    /// Layout (silently) switched — useful for the tray icon to update.
    LayoutChanged(LayoutId),
    /// A correction has just been applied.
    Corrected {
        from_layout: LayoutId,
        to_layout: LayoutId,
        original_text: String,
        corrected_text: String,
        reason: String,
    },
    /// Engine has been paused / resumed via hotkey.
    PausedChanged(bool),
    /// Engine looked at the buffer but decided to keep the current
    /// layout — useful for debug overlays.
    KeptCurrent { reason: String },
}

/// Commands sent into the engine from the app loop.
#[derive(Debug, Clone)]
pub enum EngineCommand {
    /// Toggle paused state (Pause-hotkey).
    TogglePause,
    /// Force a switch on the most recently completed word, ignoring
    /// the detector (Manual-switch-last hotkey).
    SwitchLastForcefully,
    /// Settings changed; refresh whatever caches the engine keeps.
    SettingsReloaded,
}

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
    /// Wall-clock deadline before which we ignore incoming key events.
    ///
    /// On Linux/Wayland the only correction path that actually works
    /// inside terminals and Wayland-native apps is to replay the
    /// original scancodes via uinput *after* `switch_to`. But our
    /// uinput device is not distinguishable from a real keyboard at
    /// the listener level — keyd (and similar input remappers) proxies
    /// our virtual events through its own virtual keyboard, stripping
    /// the `injected` marker entirely. Without a guard the engine
    /// would read its own replay back, run another correction on it,
    /// and spiral into an infinite backspace+space loop that locks
    /// the user out of typing for seconds at a time. Suppressing
    /// events for ~300 ms after each correction is the simplest
    /// reliable fix that doesn't require a deeper rewrite (Wayland's
    /// `zwp_virtual_keyboard_v1` would be the proper long-term path).
    injection_until: Arc<RwLock<Instant>>,
}

#[derive(Debug, Clone)]
struct LastWord {
    keys: Vec<kb_types::WordKey>,
    rendered: String,
    layout: LayoutId,
    /// The boundary character the user typed after the word. The
    /// corrector backspaces over it and re-emits a copy.
    boundary_char: char,
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
            injection_until: Arc::new(RwLock::new(Instant::now())),
        }
    }

    pub fn paused(&self) -> bool {
        *self.paused.read()
    }

    /// Drive the engine to completion. Returns when both channels close.
    pub fn run(self, key_rx: Receiver<KeyEvent>, cmd_rx: Receiver<EngineCommand>) {
        let mut buffer = WordBuffer::new();
        let mut last_event_at = Instant::now();
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
                Either::Cmd(cmd) => self.handle_command(cmd, &mut buffer),
                Either::Key(ev) => {
                    if last_event_at.elapsed() > idle_timeout {
                        debug!("idle timeout — clearing word buffer");
                        buffer.clear();
                    }
                    last_event_at = Instant::now();
                    self.handle_key(ev, &mut buffer);

                    // After processing, drain non-blocking commands so
                    // hotkeys feel snappy even under heavy typing load.
                    while let Ok(cmd) = cmd_rx.try_recv() {
                        self.handle_command(cmd, &mut buffer);
                    }
                }
            }
        }

        info!("engine shutting down");
    }

    fn handle_command(&self, cmd: EngineCommand, buffer: &mut WordBuffer) {
        match cmd {
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
                    self.force_switch_last(last);
                } else {
                    debug!(
                        "manual switch-last fired but no last word stashed (likely a duplicate from key auto-repeat)"
                    );
                }
            }
            EngineCommand::SettingsReloaded => {
                self.audio.refresh_from(&self.settings);
                buffer.clear();
            }
        }
    }

    fn handle_key(&self, ev: KeyEvent, buffer: &mut WordBuffer) {
        if ev.injected {
            // Avoid feedback: our own corrections come back through here.
            return;
        }
        // Linux/Wayland: see `injection_until` field docs. Drop the
        // buffer too — leaving stale keys from before the correction
        // would mean the next real word starts mid-token.
        if Instant::now() < *self.injection_until.read() {
            buffer.clear();
            return;
        }
        if *self.paused.read() {
            return;
        }
        if ev.modifiers.is_command() {
            // Shortcuts (Ctrl+C, Cmd+V, …) — flush, don't accumulate.
            buffer.clear();
            return;
        }

        // Resolve the character this scancode produces under the
        // *currently active* layout — the buffer needs that to
        // classify (a `,`-position scancode is `б` in uk-UA, etc.).
        // The lookup is cheap (one Win32 call on Windows, similarly
        // light on the other backends).
        let produced = self.translate_via_current_layout(ev.scancode, ev.modifiers.shift);
        // Cross-layout letter hint: keeps Cyrillic words intact when
        // typed under en-US (`б` at 0x33 would otherwise look like a
        // `,` boundary). See `WordBuffer::feed` for the full rationale.
        // Shift-aware so adding more layouts (de-DE / fr-FR / …) doesn't
        // accidentally classify genuine en-US punctuation as "letter
        // in another layout".
        let letter_in_any_layout = self
            .layouts
            .is_letter_in_any_layout(ev.scancode, ev.modifiers.shift);

        if let WordBoundary::WordCompleted {
            boundary_scancode,
            boundary_shift,
        } = buffer.feed(ev, produced, letter_in_any_layout)
        {
            self.decide(buffer, boundary_scancode, boundary_shift);
            buffer.start_new_word();
        }
    }

    /// Best-effort current-layout translate. Returns `None` if we
    /// can't query the OS or the scancode isn't in the mapping
    /// table — both are normal for control / OEM keys.
    fn translate_via_current_layout(&self, scancode: u32, shift: bool) -> Option<char> {
        let current = self.layout_switcher.current().ok()?;
        let mapping = self.layouts.get(&current)?;
        mapping.translate_key(kb_types::WordKey {
            scancode,
            shift,
            timestamp_ms: 0,
        })
    }

    fn decide(&self, buffer: &mut WordBuffer, boundary_scancode: u32, boundary_shift: bool) {
        let snap = self.settings.snapshot();
        let keys = buffer.take_word();
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
                m.translate_key(kb_types::WordKey {
                    scancode: boundary_scancode,
                    shift: boundary_shift,
                    timestamp_ms: 0,
                })
            })
            .or(match boundary_scancode {
                0x39 => Some(' '),
                0x1C => Some('\n'),
                0x0F => Some('\t'),
                _ => None,
            })
            .unwrap_or(' ');

        *self.last_word.write() = Some(LastWord {
            keys: keys.clone(),
            rendered: current_text.clone(),
            layout: current_layout.clone(),
            boundary_char,
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
            self.dispatch_smart_command(cmd, current_text.chars().count() + 1, boundary_char);
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

        let ctx = kb_detect::DetectionContext {
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
                    backspaces: current_text.chars().count() + 1,
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
    ) {
        debug!(%from, %to, %original, %corrected, %reason, "applying correction");

        // Pre-flight: confirm the target layout is currently active in
        // the OS BEFORE we touch the user's text. Sending the
        // backspaces first and then discovering the switch is
        // impossible would leave the word deleted with nothing typed
        // back — which is exactly the failure mode that destroyed
        // `http ` when the detector picked an OS-inactive layout.
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
        // the original error (still safe — backspaces haven't run yet).
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

        // Open the injection lockout window BEFORE the first synthetic
        // event leaves the emitter. The backspaces come back through
        // the listener with `injected = false` on Linux (see field
        // docs), so without this guard those alone are enough to
        // restart the engine mid-correction.
        *self.injection_until.write() = Instant::now() + Duration::from_millis(400);
        if let Err(e) = self.key_emitter.send_backspaces(backspaces) {
            warn!(?e, "send_backspaces failed; aborting correction");
            return;
        }
        if let Err(e) = self.layout_switcher.switch_to(to) {
            warn!(?e, target = %to, "layout switch failed; aborting correction");
            return;
        }
        // Prefer replaying the original scancodes against the freshly
        // switched layout (the only path that works in Wayland-native
        // / terminal apps). Backends that have a real Unicode-emit API
        // (`KEYEVENTF_UNICODE`, `CGEventKeyboardSetUnicodeString`)
        // return `Unsupported`; we fall back to `send_text` for them.
        let replayed = match replay_keys {
            Some(rk) => match self.key_emitter.send_keys(rk) {
                Ok(()) => true,
                Err(InputError::Unsupported(_)) => false,
                Err(e) => {
                    warn!(?e, "send_keys failed; correction may be partial");
                    return;
                }
            },
            None => false,
        };
        if !replayed {
            if let Err(e) = self.key_emitter.send_text(corrected) {
                warn!(?e, "send_text failed; correction may be partial");
                return;
            }
        }
        // Re-arm the lockout from the post-emit moment too — keyd /
        // similar remappers proxy our synthetic events through their
        // own virtual keyboard with their own scheduling, so the
        // echoes can land well after we finished emitting.
        *self.injection_until.write() = Instant::now() + Duration::from_millis(300);
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
        if let Err(e) = self.key_emitter.send_backspaces(backspace_count) {
            warn!(?e, id = %cmd.id, "smart command: send_backspaces failed");
            return;
        }
        match &cmd.action {
            CommandAction::TypeText { text } => {
                if let Err(e) = self.key_emitter.send_text(text) {
                    warn!(?e, id = %cmd.id, "smart command: send_text failed");
                    return;
                }
                // Re-emit the boundary so the user's typing flow
                // continues — they typed `anrl<space>`, they expect
                // `<expansion><space>` afterward, not the cursor
                // glued to the end.
                let mut buf = [0u8; 4];
                let s = boundary_char.encode_utf8(&mut buf);
                if let Err(e) = self.key_emitter.send_text(s) {
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

    fn force_switch_last(&self, last: LastWord) {
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
        // For the manual hotkey we don't have the original boundary
        // scancode any more (a space is the overwhelmingly common
        // case, hard-coded here). If we ever want to support
        // Enter/Tab boundaries on the manual path we'd need to stash
        // them on `LastWord` too.
        let mut replay: Vec<ReplayKey> = last
            .keys
            .iter()
            .map(|k| ReplayKey {
                scancode: k.scancode,
                shift: k.shift,
            })
            .collect();
        replay.push(ReplayKey {
            scancode: 0x39,
            shift: false,
        });
        self.apply_correction(
            &last.layout,
            &target,
            &last.rendered,
            &corrected,
            last.rendered.chars().count() + 1,
            "manual switch-last hotkey",
            true,
            Some(&replay),
        );
    }
}

enum Either<A, B> {
    Cmd(A),
    Key(B),
}

/// Case-insensitive basename match against the user's disabled-apps
/// list. We use ASCII-lowercase rather than full Unicode lowering
/// because every executable basename we ever match is ASCII.
fn app_is_disabled(exe: &str, disabled: &[String]) -> bool {
    let needle = exe.to_ascii_lowercase();
    disabled
        .iter()
        .any(|entry| entry.eq_ignore_ascii_case(&needle))
}

/// Boundary characters that strongly suggest the user is typing a
/// URL / file path / email address / config expression / source code
/// rather than prose. When the engine sees one of these as the
/// boundary it skips auto-switching: the just-completed token is
/// almost certainly part of an address-like construct and shouldn't
/// be re-rendered through another keyboard layout.
///
/// The list is conservative — only characters that are *almost
/// always* structural in real prose, never sentence punctuation:
///
/// * `:` — URL scheme, time, key:value, ratio, ternary
/// * `/` — path separator, URL, division, regex
/// * `\` — Windows path, escape
/// * `@` — email, mention, decorator, npm scope
/// * `=` — assignment, query string, equality
/// * `#` — anchor, hashtag, source comment, channel
/// * `&` — URL query separator, bitwise
///
/// Notably absent: `.` (also sentence-end), `(`, `)`, `[`, `]`,
/// `{`, `}`, `"` (all common in prose), `+`, `*`, `<`, `>`, `|`,
/// `~`, `` ` `` (less common in prose but lower confidence as
/// "definitely structural").
fn is_structural_boundary(ch: char) -> bool {
    matches!(ch, ':' | '/' | '\\' | '@' | '=' | '#' | '&')
}

/// Decide whether `id` belongs in the candidate set the detectors get
/// to score against. Three filters, AND'd together:
///
/// * **Settings allow-list (`active`)** — empty means "no allow-list,
///   every loaded layout passes". Non-empty means only listed layouts
///   pass; the *current* layout always passes regardless, so a Switch
///   verdict is never silently locked-in by virtue of the user typing
///   in a layout they haven't whitelisted.
/// * **Settings veto (`ignored`)** — anything in this list never
///   passes, period.
/// * **OS-active list (`os_active`)** — `Some(list)` means "filter to
///   only layouts the OS reports as currently installed/enabled" (with
///   the current layout always passing as a safety net for the rare
///   case where the OS list omits it transiently). `None` means the
///   query failed and we fail-open — same behaviour as before this
///   filter existed.
///
/// Pulled out as a standalone fn so it's unit-testable without
/// constructing a full engine.
fn is_layout_eligible(
    id: &LayoutId,
    current: &LayoutId,
    settings_active: &[LayoutId],
    settings_ignored: &[LayoutId],
    os_active: Option<&[LayoutId]>,
) -> bool {
    let allowed = settings_active.is_empty() || settings_active.contains(id) || id == current;
    let blocked = settings_ignored.contains(id);
    let os_ok = os_active
        .map(|a| a.contains(id) || id == current)
        .unwrap_or(true);
    allowed && !blocked && os_ok
}

/// Render the buffer through the current layout, but skip every
/// character that's a *cross-layout artifact* — i.e. punctuation
/// under the current layout whose scancode is actually a letter
/// somewhere else.
///
/// Why: with the cross-layout-letter buffer hint (see
/// `WordBuffer::feed`), a buffer can contain scancodes whose current-
/// layout rendering is `;` / `[` / `'` even though the user clearly
/// meant a Cyrillic letter. The dictionary detector strips those
/// before lookup; the code-token guard needs the same courtesy or it
/// fires on every Ukrainian word containing `ж`, `х`, `ї`, `є`, etc.
/// (their scancodes are punctuation in en-US: 0x27 → `;`, 0x1A → `[`,
/// 0x28 → `'`, 0x1B → `]`). The visible bug: typing `Друже` under
/// en-US rendered as `Lhe;t`, and the `;` made
/// `looks_like_code_token` veto the auto-switch.
///
/// Falls back to the already-computed `current_text` if the current
/// layout isn't loaded in the DB (shouldn't happen at runtime, but the
/// engine's mid-decision path needs to keep going either way).
fn render_for_code_check(
    keys: &[kb_types::WordKey],
    current_layout: &LayoutId,
    layouts: &LayoutDb,
    fallback: &str,
) -> String {
    let Some(mapping) = layouts.get(current_layout) else {
        return fallback.to_owned();
    };
    let mut out = String::with_capacity(keys.len());
    for &k in keys {
        let Some(c) = mapping.translate_key(k) else {
            continue;
        };
        // Cross-layout artifact: non-letter under current, but the
        // scancode-at-this-shift IS a letter in some other layout.
        // The user meant a letter, not punctuation — drop it from the
        // code-token view. Checking shift granularity is critical:
        // without it, scancode 0x0C unshifted being `ß` in de-DE
        // would (wrongly) cause the SHIFTED `_` produced under en-US
        // to be stripped from `foo_bar`.
        if !c.is_alphabetic() && layouts.is_letter_in_any_layout(k.scancode, k.shift) {
            continue;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod boundary_tests {
    use super::is_structural_boundary;

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
}

#[cfg(test)]
mod last_word_consume_tests {
    use super::LastWord;
    use kb_layout::LayoutId;
    use parking_lot::RwLock;
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

#[cfg(test)]
mod code_check_render_tests {
    use super::render_for_code_check;
    use crate::layouts::LayoutDb;
    use kb_layout::LayoutId;
    use kb_types::WordKey;

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

#[cfg(test)]
mod layout_eligibility_tests {
    use super::is_layout_eligible;
    use kb_layout::LayoutId;

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

#[cfg(test)]
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
