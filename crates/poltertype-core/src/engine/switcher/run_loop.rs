//! The engine's run loop: channel multiplexing plus the top-level
//! command and key dispatch.

use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, select_biased};
use poltertype_input::{KeyDirection, KeyEvent};
use tracing::{debug, info};

use crate::audio::SoundEvent;
use crate::engine::buffer::{WordBoundary, WordBuffer};
use crate::engine::consts::PASTE_GUARD;
use crate::engine::enums::{Either, EngineCommand, SwitcherEvent};
use crate::engine::heuristics::is_paste_shortcut;
use crate::engine::types::ChordState;

use super::engine::SwitcherEngine;

impl SwitcherEngine {
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

    pub(super) fn handle_command(
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

    pub(super) fn handle_key(
        &self,
        ev: KeyEvent,
        buffer: &mut WordBuffer,
        key_rx: &Receiver<KeyEvent>,
    ) {
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
}
