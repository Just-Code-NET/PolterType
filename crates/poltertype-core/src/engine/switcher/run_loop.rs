//! The engine's run loop: channel multiplexing plus the top-level
//! command and key dispatch.

use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, select_biased};
use poltertype_input::{KeyDirection, KeyEvent};
use tracing::{debug, info};

use crate::audio::SoundEvent;
use crate::engine::buffer::{WordBoundary, WordBuffer};
use crate::engine::consts::{FORCE_SWITCH_REARM, LAST_WORD_TTL, PASTE_GUARD, SC_BACKSPACE};
use crate::engine::enums::{Either, EngineCommand, SwitcherEvent};
use crate::engine::heuristics::{is_modifier_scancode, is_paste_shortcut};

use super::engine::SwitcherEngine;

impl SwitcherEngine {
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
                    self.check_keystream_hotkeys(&ev, &mut buffer, &key_rx);
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
            EngineCommand::SetPaused(want) => {
                if *self.paused.read() != want {
                    self.handle_command(EngineCommand::TogglePause, buffer, key_rx);
                }
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
                // Every fire our own correction provokes is stopped
                // here, and only here. `force_switch_last` emits
                // Backspaces, which Win32 `RegisterHotKey` reads
                // together with the user's still-held Ctrl+Shift as a
                // fresh press; auto-repeat does the same without the
                // modifier edge. That used to be absorbed by taking the
                // stash atomically and leaving every repeat with
                // `None` — `wow ` had accumulated to `wow wow wow…`
                // until the app was killed — but the stash is now put
                // back so the hotkey can be pressed twice (issue #37),
                // and a window is what tells an echo from a person.
                if let Some(t) = *self.last_force_switch.read()
                    && t.elapsed() < FORCE_SWITCH_REARM
                {
                    debug!("manual switch-last ignored: within the re-arm window of the last one");
                    return;
                }
                // Which word the caret is sitting after decides the
                // rest. `completed() + boundary_run() + keys()` is the
                // buffer's model of the text left of the caret, and a
                // correction backspaces *from the caret* — so switching
                // anything but the last item of that model erases
                // whatever follows it instead. A hotkey pressed a
                // moment late, once the next word had started, did
                // exactly that: the previous word's backspace count
                // landing several characters too far right, and a line
                // left in pieces.
                let in_progress = !buffer.keys().is_empty();
                let taken = if in_progress {
                    self.word_in_progress(buffer)
                } else if buffer.boundary_run().len() <= 1 {
                    self.last_word.write().take()
                } else {
                    // More separators than the one that closed the
                    // word: the caret is past them, and we cannot put
                    // back what we never measured.
                    debug!(
                        separators = buffer.boundary_run().len(),
                        "manual switch-last: the caret has moved past the stashed word"
                    );
                    None
                };
                if let Some(word) = taken {
                    // The force-switch replays the same scancodes, so
                    // the pending offer's identity check would still
                    // pass and a late click would replace the
                    // transliterated word with the old word's suggestion.
                    self.dismiss_suggestions(None);
                    // Taking the stash is how we got here, and a press
                    // that ends up doing nothing — most often because
                    // the key is still down — must not have eaten it.
                    // A word still in the buffer needs no copy: it was
                    // never taken from anywhere.
                    let stashed = (!in_progress).then(|| word.clone());
                    if !self.force_switch_last(word, buffer, key_rx) {
                        // Unless the buffer was tainted on the way out,
                        // which says the caret is no longer where the
                        // stash thinks it is.
                        if let Some(w) = stashed
                            && !buffer.poisoned()
                        {
                            *self.last_word.write() = Some(w);
                        }
                        return;
                    }
                    if in_progress {
                        // The user has just settled this word's layout
                        // by hand. Its keys are still in the buffer and
                        // would get a second opinion at the boundary —
                        // one that can only disagree with them. This
                        // taints exactly that completion; `abandon`,
                        // which used to stand here, also told the
                        // buffer the caret was lost, and a lost caret
                        // is refused by `word_in_progress` — so the
                        // hotkey went dead for every word typed
                        // afterwards until a separator cleared it
                        // (issue #40).
                        buffer.settle();
                    }
                } else if self.settings.snapshot().selection.enabled {
                    // No word to act on is exactly the shape of "the
                    // user selected something instead", and the only
                    // moment worth spending a `Ctrl+C` on. Off by
                    // default; see `SelectionSettings`.
                    if self.convert_selection() {
                        *self.last_force_switch.write() = Some(Instant::now());
                    }
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
        // No paused early-return here. Pause is about *auto*-switching,
        // and a buffer that stops tracking while it is on takes the
        // manual hotkey down with it: the stash is written at a word
        // boundary, so nothing typed during a pause was ever reachable
        // by it (issue #36). The decision itself is what pause stops —
        // see `decide`.
        //
        // Opens a window during which we decline to auto-correct — see
        // `paste_guard_until`.
        if is_paste_shortcut(&ev) {
            *self.paste_guard_until.write() = Instant::now() + PASTE_GUARD;
        }
        if self.is_own_switch_press(&ev) {
            // The force-switch chord's own key, which by now has
            // already run its correction and knows precisely where the
            // caret is. Feeding it to the buffer taints the word it
            // just switched — which left the gesture dead for every
            // word typed after it (issue #40) — and for a binding with
            // no modifier on it there is nothing below to catch it at
            // all: the classifier reads a bare function key as the
            // caret moving, so the press threw away the very word it
            // had switched and the next press found nothing.
            self.dismiss_suggestions(None);
            return;
        }
        if ev.scancode == SC_BACKSPACE && ev.modifiers.is_command() {
            // A word- or line-delete to the left, and the one shortcut
            // whose effect on the text we know: it erases what sat left
            // of the caret. Reading it as an arbitrary edit taints the
            // *next* word too, and that is what read as the hotkey
            // dying after a line was cleared (issue #44). The stash
            // goes with it — the word it names is off the screen now.
            buffer.delete_word_left();
            *self.last_word.write() = None;
            self.dismiss_suggestions(None);
            return;
        }
        if ev.modifiers.is_command() && !is_modifier_scancode(ev.scancode) {
            // Shortcuts can edit text arbitrarily, so a mid-flight word
            // is no longer trustworthy. The stashed last-word survives
            // — the manual switch-last chord is itself a shortcut.
            //
            // Bare modifier presses are exempt (`is_modifier_scancode`):
            // the suggestion-accept chord must survive its own
            // modifiers, and the digit that follows is what accepts.
            //
            // The pause chord is deliberately not exempt: its default
            // key is Space, which the buffer reads as a word boundary.
            buffer.abandon();
            // A shortcut can also move the caret (Ctrl+End,
            // app-specific jumps), so the next word may start mid-word.
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
