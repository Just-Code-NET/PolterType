//! The suggestion tooltip's claimed keys: `F1`..`F9` and the
//! `<modifiers>+digit` chord accept by number, `Down` / `Up` move a
//! highlight and `Enter` accepts it.
//!
//! Every key handled here was kept from the application by the
//! listener (`KeyGate::claim`), so none of it is typing or a caret move
//! and none of it may reach the word buffer. Where the listener cannot
//! claim, an offer never turns the F-keys or arrows on, and the digit
//! chord is matched off the key stream as before — see
//! `check_suggestion_chord`.

use std::time::Instant;

use crossbeam_channel::Receiver;
use poltertype_input::{ClaimKey, ClaimSet, KeyDirection, KeyEvent, Modifiers};

use crate::engine::buffer::WordBuffer;
use crate::engine::enums::{EngineCommand, SwitcherEvent};
use crate::engine::types::PendingSuggestion;

use super::engine::SwitcherEngine;

impl SwitcherEngine {
    /// Claim the keys `pending` answers to, until its deadline.
    ///
    /// `Enter` only once a row is highlighted: until then it belongs to
    /// the application, and a tooltip that ate the newline the user was
    /// typing would be the worst thing it could do.
    ///
    /// The digit chord too, with its modifiers: left to reach the
    /// application, `Ctrl+Shift+1` types a character in some (xterm)
    /// and nothing in others, and the replacement cannot know how many
    /// to erase.
    pub(super) fn claim_suggestion_keys(&self, pending: &PendingSuggestion) {
        let mut keys = ClaimSet::default();
        if let Some(a) = pending.accept {
            keys = keys.with_digit_modifiers(Modifiers {
                control: a.ctrl,
                shift: a.shift,
                alt: a.alt,
                meta: a.meta,
                ..Modifiers::NONE
            });
            for n in 1..=pending.entries.len().min(9) {
                keys = keys.with(ClaimKey::Digit(n as u8));
            }
        }
        if pending.function_keys {
            for n in 1..=pending.entries.len().min(9) {
                keys = keys.with(ClaimKey::Function(n as u8));
            }
        }
        if pending.arrows {
            keys = keys.with(ClaimKey::Up).with(ClaimKey::Down);
            if pending.highlight.is_some() {
                keys = keys.with(ClaimKey::Enter);
            }
        }
        if keys.is_empty() {
            self.key_gate.release_claim();
        } else {
            self.key_gate.claim(keys, pending.deadline);
        }
    }

    /// Act on a keystroke the listener kept back for the tooltip.
    pub(super) fn handle_claimed_key(
        &self,
        ev: &KeyEvent,
        buffer: &mut WordBuffer,
        key_rx: &Receiver<KeyEvent>,
    ) {
        if ev.direction == KeyDirection::Release {
            // Clears the latch an accept below set.
            self.track_trigger_key(ev);
            return;
        }
        let target = match ClaimKey::from_scancode(ev.scancode) {
            Some(ClaimKey::Down) => {
                self.move_suggestion_highlight(true);
                None
            }
            Some(ClaimKey::Up) => {
                self.move_suggestion_highlight(false);
                None
            }
            Some(ClaimKey::Enter) => {
                self.claimed_accept_target(|p| p.highlight.filter(|_| p.arrows))
            }
            Some(ClaimKey::Function(n)) => {
                self.claimed_accept_target(|p| p.function_keys.then(|| usize::from(n) - 1))
            }
            Some(ClaimKey::Digit(n)) => {
                self.claimed_accept_target(|p| p.accept.map(|_| usize::from(n) - 1))
            }
            None => None,
        };
        let Some((generation, index)) = target else {
            return;
        };
        // The replacement waits for this key to come up, as it does for
        // a hotkey's: on X11 the grab that kept it back delivers every
        // keystroke to us until its release — our own typing included.
        // A digit waits for its chord's modifiers too.
        let accept_mods = matches!(
            ClaimKey::from_scancode(ev.scancode),
            Some(ClaimKey::Digit(_))
        )
        .then(|| {
            self.pending_suggestion
                .lock()
                .as_ref()
                .and_then(|p| p.accept)
        })
        .flatten();
        {
            let mut st = self.chord_state.lock();
            st.trigger_down = Some(ev.scancode);
            st.accept_mods = accept_mods;
        }
        self.handle_command(
            EngineCommand::AcceptSuggestion {
                generation,
                index,
                from_pointer: false,
            },
            buffer,
            key_rx,
        );
    }

    /// First `Down` highlights the first row, first `Up` the last;
    /// after that they step, wrapping at either end. A user reading the
    /// list is not idle, so the offer's lifetime restarts.
    fn move_suggestion_highlight(&self, down: bool) {
        let moved = {
            let mut slot = self.pending_suggestion.lock();
            let Some(p) = slot.as_mut() else {
                return;
            };
            let n = p.entries.len();
            let now = Instant::now();
            if !p.arrows || n == 0 || now > p.deadline {
                return;
            }
            let next = match (p.highlight, down) {
                (None, true) => 0,
                (None, false) => n - 1,
                (Some(i), true) => (i + 1) % n,
                (Some(i), false) => (i + n - 1) % n,
            };
            p.highlight = Some(next);
            p.deadline = now + self.settings.snapshot().suggestions.timeout();
            self.claim_suggestion_keys(p);
            (p.generation, next)
        };
        let (generation, index) = moved;
        let _ = self
            .out_tx
            .send(SwitcherEvent::SuggestionHighlighted { generation, index });
    }

    /// The live offer and the row `pick` chooses from it, if both
    /// exist.
    fn claimed_accept_target(
        &self,
        pick: impl FnOnce(&PendingSuggestion) -> Option<usize>,
    ) -> Option<(u64, usize)> {
        let slot = self.pending_suggestion.lock();
        let p = slot.as_ref()?;
        let index = pick(p)?;
        (index < p.entries.len()).then_some((p.generation, index))
    }
}
