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
use crate::engine::heuristics::{is_modifier_scancode, is_paste_shortcut};
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
                    // Our own injected keystrokes echoing back
                    // (Linux behind keyd & friends) — swallowed before
                    // anything else can act on them.
                    if self.consume_echo(&ev) {
                        last_event_at = Instant::now();
                        continue;
                    }
                    // Remember what the user is holding: a correction
                    // fired by a chord must let those keys go before
                    // typing, or the replay lands as shortcuts.
                    *self.held_modifiers.write() = ev.modifiers;
                    // Click-grace bookkeeping first: a frozen offer
                    // (pointer press seen, tooltip click possibly in
                    // flight) dies on the first real keypress or when
                    // its window lapses.
                    self.click_grace_tick(&ev);
                    self.check_keystream_hotkeys(&ev, &mut chord_state, &mut buffer, &key_rx);
                    if last_event_at.elapsed() > idle_timeout {
                        // A live offer overrides idle hygiene while
                        // no word is mid-flight: the tooltip promises
                        // the word stays replaceable, and pausing to
                        // read it is the expected interaction. Anything
                        // that really invalidates the caret arrives as
                        // its own event and dismisses through the
                        // normal paths.
                        if self.has_live_suggestion() && buffer.keys().is_empty() {
                            debug!("idle timeout skipped — live suggestion offer");
                        } else {
                            debug!("idle timeout — abandoning word buffer");
                            // Not a plain clear: with a word
                            // mid-flight the screen still holds its
                            // head, and correcting only the tail would
                            // chop it in half. A pending offer dies
                            // with the buffer.
                            buffer.abandon();
                            *self.last_word.write() = None;
                            // Bounded by time as well as length: a
                            // machine left alone must not still hold a
                            // sentence, and a trigger must not fire
                            // from words typed before a long pause.
                            self.word_history.write().clear();
                            self.dismiss_suggestions(None);
                        }
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
                let now = {
                    let mut g = self.paused.write();
                    *g = !*g;
                    *g
                };
                info!(paused = now, "pause toggled");
                if now {
                    self.dismiss_suggestions(None);
                }
                let _ = self.out_tx.send(SwitcherEvent::PausedChanged(now));
                self.audio.play(if now {
                    SoundEvent::Pause
                } else {
                    SoundEvent::Resume
                });
            }
            EngineCommand::SwitchLastForcefully => {
                // Atomic take, NOT clone-and-read — this is what stops
                // the hotkey loop.
                //
                // `force_switch_last` emits Backspaces. They are
                // flagged injected so the engine ignores them, but the
                // OS-level `RegisterHotKey` sees our Backspace plus the
                // user's still-held Ctrl+Shift as a fresh press and
                // fires again — correcting the text and playing the
                // sound each time, so `wow ` accumulated to `wow wow
                // wow…` until the app was killed. Auto-repeat on a held
                // Backspace does the same without the modifier edge.
                //
                // Taking atomically means every subsequent fire from
                // the same press finds `None` and exits. Re-triggering
                // needs another completed word.
                let taken = self.last_word.write().take();
                if let Some(last) = taken {
                    // The pending offer was computed for the
                    // pre-switch rendering, and the force-switch replays
                    // the same scancodes — so the identity check would
                    // still pass and a late click would replace the
                    // transliterated word with the old word's suggestion.
                    self.dismiss_suggestions(None);
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
                self.dismiss_suggestions(None);
            }
            EngineCommand::AcceptSuggestion {
                generation,
                index,
                typed_digit,
                from_pointer,
            } => {
                self.accept_suggestion(
                    generation,
                    index,
                    typed_digit,
                    from_pointer,
                    buffer,
                    key_rx,
                );
            }
            EngineCommand::DismissSuggestions { generation } => {
                self.dismiss_suggestions(Some(generation));
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
        if ev.modifiers.is_command() && !is_modifier_scancode(ev.scancode) {
            // Shortcuts (Ctrl+C, Cmd+V, …) can edit text arbitrarily,
            // so a mid-flight word is no longer trustworthy: taint it,
            // and a pending offer dies with it. The stashed last-word
            // survives — the manual switch-last chord is itself a
            // shortcut.
            //
            // Bare modifier presses are exempt (`is_modifier_scancode`):
            // the suggestion-accept chord must survive its own
            // modifiers, and the digit that follows is what accepts.
            buffer.abandon();
            // A shortcut can also move the caret (Ctrl+End, Cmd+click
            // chords, app-specific jumps) — the next word may start
            // mid-word.
            buffer.mark_context_unclean();
            self.dismiss_suggestions(None);
            return;
        }

        // A pointer press is about to abandon the buffer below — if a
        // suggestion tooltip is up, freeze the screen model first so
        // a click ON the tooltip (whose Accepted event arrives via
        // the command channel a moment later) can still be honoured.
        if ev.direction == KeyDirection::Press && ev.scancode == poltertype_types::SC_POINTER_BUTTON
        {
            self.freeze_suggestion_for_click(buffer);
        }

        // Cross-layout letter hint: keeps Cyrillic words intact under
        // en-US (`б` at 0x33 would otherwise read as a `,` boundary).
        // Shift-aware, so adding layouts cannot reclassify genuine
        // en-US punctuation. See `WordBuffer::feed`.
        let letter_in_any_layout = self
            .layouts
            .is_letter_in_any_layout(ev.scancode, ev.modifiers.shift);
        // The character this scancode produces under the *currently
        // active* layout, which the buffer needs to classify. Only when
        // classification depends on it — the cross-layout hint settles
        // letters, and releases never reach the classifier.
        let produced = if ev.direction == KeyDirection::Press && !letter_in_any_layout {
            self.translate_via_current_layout(ev.scancode, ev.modifiers.shift)
        } else {
            None
        };

        match buffer.feed(ev, produced, letter_in_any_layout) {
            WordBoundary::InProgress => {}
            WordBoundary::Abandoned => {
                // Caret went somewhere unknown (click / nav / Esc), so
                // a stash would now be corrected at the wrong position.
                // Same for a pending offer — except inside the click
                // grace window, where this abandon may have come from a
                // pointer press that landed on the tooltip.
                *self.last_word.write() = None;
                if !self.has_click_grace() {
                    self.dismiss_suggestions(None);
                }
            }
            WordBoundary::WordCompleted {
                boundary_scancode,
                boundary_shift,
                tainted,
                started_clean,
            } => {
                // Whatever happens to this word, the previous word's
                // offer no longer points at the last thing on screen —
                // `decide()` below may immediately issue a fresh one.
                self.dismiss_suggestions(None);
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
                    self.decide(
                        buffer,
                        boundary_scancode,
                        boundary_shift,
                        started_clean,
                        key_rx,
                    );
                }
            }
        }
    }
}
