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
use kb_detect::Detector;
use kb_input::{KeyEmitter, KeyEvent};
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
    pub fn new(
        settings: Arc<SettingsStore>,
        layouts: Arc<LayoutDb>,
        detectors: Vec<Box<dyn Detector>>,
        layout_switcher: Arc<dyn LayoutSwitcher>,
        key_emitter: Arc<dyn KeyEmitter>,
        audio: Arc<AudioPlayer>,
        out_tx: Sender<SwitcherEvent>,
    ) -> Self {
        Self {
            settings,
            layouts,
            detectors,
            layout_switcher,
            key_emitter,
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

        if let WordBoundary::WordCompleted {
            boundary_scancode,
            boundary_shift,
        } = buffer.feed(ev)
        {
            self.decide(buffer, boundary_scancode, boundary_shift);
            buffer.start_new_word();
        }
    }

    fn decide(&self, buffer: &mut WordBuffer, boundary_scancode: u32, boundary_shift: bool) {
        let snap = self.settings.snapshot();
        let keys = buffer.take_word();
        if keys.is_empty() {
            return;
        }
        if keys.len() < snap.engine.min_word_length {
            return;
        }

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

        // Stash the rendered current text for "switch-last" later.
        let current_text = candidates
            .iter()
            .find(|(l, _)| l == &current_layout)
            .map(|(_, t)| t.clone())
            .unwrap_or_default();

        // Resolve the boundary scancode → character under the current
        // layout (so a Ukrainian "." lands as ".", a comma as ",", …).
        // Falls back to a hard-coded table for keys our TOMLs don't
        // describe (Tab/Enter/Space).
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

        let ctx = kb_detect::DetectionContext {
            current_layout: &current_layout,
            candidates: &candidates,
            recent_context: "",
        };

        // First detector with confidence ≥ threshold wins.
        let verdict = self
            .detectors
            .iter()
            .find_map(|d| d.detect(&ctx))
            .filter(|v| v.confidence >= snap.engine.confidence_threshold);

        let action = match verdict {
            Some(v) => {
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
            None => SwitchAction::KeepCurrent,
        };

        match action {
            SwitchAction::KeepCurrent => {
                let _ = self.out_tx.send(SwitcherEvent::KeptCurrent {
                    reason: "no detector cleared threshold".into(),
                });
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
