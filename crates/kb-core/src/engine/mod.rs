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
use kb_input::{FocusTracker, KeyEmitter, KeyEvent};
use kb_layout::{LayoutId, LayoutSwitcher};
use kb_types::SwitchAction;
use parking_lot::RwLock;
use tracing::{debug, info, warn};

use crate::audio::{AudioPlayer, SoundEvent};
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
                if let Some(last) = self.last_word.read().clone() {
                    self.force_switch_last(last);
                } else {
                    warn!("no last word to switch");
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

        if let WordBoundary::WordCompleted {
            boundary_scancode,
            boundary_shift,
        } = buffer.feed(ev, produced)
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

        // Filter the candidate set by the `[languages]` settings.
        // Empty `active` = "every loaded layout"; `ignored` always wins.
        let active: &[LayoutId] = &snap.languages.active;
        let ignored: &[LayoutId] = &snap.languages.ignored;
        let candidates: Vec<(LayoutId, String)> = self
            .layouts
            .iter()
            .filter(|(id, _)| {
                let allowed = active.is_empty() || active.contains(id) || **id == current_layout;
                let blocked = ignored.contains(id);
                allowed && !blocked
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
        if snap.engine.suppress_in_identifiers && looks_like_code_token(&current_text) {
            debug!(token = %current_text, "skipping auto-switch: looks like code identifier");
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
                self.apply_correction(
                    &current_layout,
                    &target_layout,
                    &current_text,
                    &corrected_text,
                    backspaces,
                    &reason,
                    snap.general.sound_on_correct,
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
    ) {
        debug!(%from, %to, %original, %corrected, %reason, "applying correction");
        if let Err(e) = self.key_emitter.send_backspaces(backspaces) {
            warn!(?e, "send_backspaces failed; aborting correction");
            return;
        }
        if let Err(e) = self.layout_switcher.switch_to(to) {
            warn!(?e, target = %to, "layout switch failed; aborting correction");
            return;
        }
        if let Err(e) = self.key_emitter.send_text(corrected) {
            warn!(?e, "send_text failed; correction may be partial");
            return;
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
        self.apply_correction(
            &last.layout,
            &target,
            &last.rendered,
            &corrected,
            last.rendered.chars().count() + 1,
            "manual switch-last hotkey",
            true,
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
