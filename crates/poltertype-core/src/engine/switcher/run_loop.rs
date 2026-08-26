//! The engine's run loop: channel multiplexing plus the top-level
//! command and key dispatch.

use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, select_biased};
use poltertype_input::{KeyDirection, KeyEvent};
use tracing::{debug, info};

use crate::audio::SoundEvent;
use crate::engine::buffer::{WordBoundary, WordBuffer};
use crate::engine::consts::{LAST_WORD_TTL, PASTE_GUARD};
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
                    // Our own echoes (Linux behind keyd & friends),
                    // swallowed before anything else can act on them.
                    if self.consume_echo(&ev) {
                        last_event_at = Instant::now();
                        continue;
                    }
                    *self.held_modifiers.write() = ev.modifiers;
                    self.click_grace_tick(&ev);
                    self.check_keystream_hotkeys(&ev, &mut chord_state, &mut buffer, &key_rx);
                    if last_event_at.elapsed() > idle_timeout {
                        // A live offer overrides idle hygiene while no
                        // word is mid-flight: pausing to read the
                        // tooltip is the expected interaction, and
                        // anything that really invalidates the caret
                        // dismisses through its own path.
                        if self.has_live_suggestion() && buffer.keys().is_empty() {
                            debug!("idle timeout skipped — live suggestion offer");
                        } else {
                            debug!("idle timeout — abandoning word buffer");
                            // `abandon`, not a plain clear: with a word
                            // mid-flight the screen still holds its
                            // head, and correcting only the tail would
                            // chop it in half.
                            buffer.abandon();
                            // The stash outlives it, up to its own
                            // window: the manual switch-last hotkey is
                            // what the user reaches for *because* the
                            // automatic pass did not fire, and the
                            // chord's own Ctrl press is a key event
                            // arriving after exactly this pause.
                            // Clearing here made the hotkey a no-op for
                            // every press slower than two seconds.
                            if last_event_at.elapsed() > LAST_WORD_TTL {
                                *self.last_word.write() = None;
                            }
                            // A machine left alone must not still hold
                            // a sentence, and a trigger must not fire
                            // from words typed before a long pause.
                            self.word_history.write().clear();
                            self.dismiss_suggestions(None);
                        }
                    }
                    last_event_at = Instant::now();
                    self.handle_key(ev, &mut buffer, &key_rx);

                    // Drain pending commands so hotkeys stay snappy
                    // under heavy typing load.
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
                // Atomic take, NOT clone-and-read: the OS-level
                // `RegisterHotKey` reads the Backspaces `force_switch_last`
                // emits, plus the user's still-held Ctrl+Shift, as a
                // fresh press and fires again — `wow ` accumulated to
                // `wow wow wow…` until the app was killed. Auto-repeat
                // does the same without the modifier edge. Taking
                // atomically leaves every repeat fire with `None`.
                let taken = self.last_word.write().take();
                if let Some(last) = taken {
                    // The force-switch replays the same scancodes, so
                    // the pending offer's identity check would still
                    // pass and a late click would replace the
                    // transliterated word with the old word's suggestion.
                    self.dismiss_suggestions(None);
                    self.force_switch_last(last, buffer, key_rx);
                } else if let Some(current) = self.word_in_progress(buffer) {
                    self.dismiss_suggestions(None);
                    self.force_switch_last(current, buffer, key_rx);
                    // The user has just settled this word's layout by
                    // hand. Its keys are still in the buffer and would
                    // get a second opinion at the boundary — one that
                    // can only disagree with them. `abandon` taints
                    // exactly the next completion, which is this word.
                    //
                    // It is also what stops a held chord from switching
                    // the same word over and over: the stash above is
                    // taken atomically for that reason, and a fallback
                    // that reads the buffer would hand every repeat the
                    // same word back. Emptying it is the same guard.
                    buffer.abandon();
                } else {
                    debug!(
                        "manual switch-last fired with no word to switch (empty buffer, or a duplicate from key auto-repeat)"
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

    /// Feed one keystroke into the word buffer, stamping each new word
    /// with the layout it is being typed under.
    ///
    /// Every path that grows the buffer goes through here — the run loop
    /// and the post-correction re-seed alike — because a word that
    /// starts without a stamp inherits the previous one's and reads as a
    /// layout change that never happened. See
    /// [`SwitcherEngine::word_layout`].
    pub(super) fn feed_buffer(&self, ev: KeyEvent, buffer: &mut WordBuffer) -> WordBoundary {
        // Shift-aware, so adding layouts cannot reclassify genuine
        // en-US punctuation. See `WordBuffer::feed`.
        let letter_in_any_layout = self
            .layouts
            .is_letter_in_any_layout(ev.scancode, ev.modifiers.shift);
        // Only computed when classification depends on it: the
        // cross-layout hint settles letters, and releases never reach
        // the classifier.
        let produced = if ev.direction == KeyDirection::Press && !letter_in_any_layout {
            self.translate_via_current_layout(ev.scancode, ev.modifiers.shift, ev.modifiers.caps)
        } else {
            None
        };

        let was_empty = buffer.keys().is_empty();
        let outcome = buffer.feed(ev, produced, letter_in_any_layout);
        if was_empty && !buffer.keys().is_empty() {
            *self.word_layout.write() = self.layout_switcher.current().ok();
        }
        outcome
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
        // Opens a window during which we decline to auto-correct — see
        // `paste_guard_until`.
        if is_paste_shortcut(&ev) {
            *self.paste_guard_until.write() = Instant::now() + PASTE_GUARD;
        }
        if ev.modifiers.is_command() && !is_modifier_scancode(ev.scancode) {
            // Shortcuts can edit text arbitrarily, so a mid-flight word
            // is no longer trustworthy. The stashed last-word survives
            // — the manual switch-last chord is itself a shortcut.
            //
            // Bare modifier presses are exempt (`is_modifier_scancode`):
            // the suggestion-accept chord must survive its own
            // modifiers, and the digit that follows is what accepts.
            buffer.abandon();
            // A shortcut can also move the caret (Ctrl+End, app-specific
            // jumps), so the next word may start mid-word.
            buffer.mark_context_unclean();
            self.dismiss_suggestions(None);
            return;
        }

        // A pointer press is about to abandon the buffer below — freeze
        // the screen model first, so a click ON the tooltip (whose
        // Accepted event arrives via the command channel a moment
        // later) can still be honoured.
        if ev.direction == KeyDirection::Press && ev.scancode == poltertype_types::SC_POINTER_BUTTON
        {
            self.freeze_suggestion_for_click(buffer);
        }

        match self.feed_buffer(ev, buffer) {
            WordBoundary::InProgress => {}
            WordBoundary::Abandoned => {
                // The caret is somewhere unknown, so a stash would be
                // corrected at the wrong position. Same for a pending
                // offer — except inside the click grace window, where
                // this abandon may be a press that hit the tooltip.
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
                    // Almost certainly pasted text replayed as
                    // keystrokes, not typing.
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
