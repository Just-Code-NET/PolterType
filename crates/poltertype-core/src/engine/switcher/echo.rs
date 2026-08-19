//! Match-and-consume tracking of our own injected keystrokes echoing
//! back through the listener (Linux behind keyd & friends). See the
//! `expected_echo` field docs for the full rationale.

use std::time::{Duration, Instant};

use poltertype_input::{EmittedKey, KeyDirection, KeyEvent};

use super::engine::SwitcherEngine;

impl SwitcherEngine {
    /// Record presses the emitter just put on the wire so their
    /// echoes can be consumed off the key stream.
    pub(super) fn push_echoes(&self, emitted: Vec<EmittedKey>) {
        if emitted.is_empty() {
            return;
        }
        // Keep this tight: a stale entry that outlives its echo eats a
        // real user press of the same scancode. (`apply_correction`
        // also waits the queue out right after emitting, so entries
        // rarely live past ~100 ms.)
        let deadline = Instant::now() + Duration::from_millis(800);
        let mut q = self.expected_echo.lock();
        q.extend(
            emitted
                .iter()
                .filter(|e| e.direction == KeyDirection::Press)
                .map(|e| (e.scancode, deadline)),
        );
        // A runaway queue must never eat minutes of real typing.
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
    pub(super) fn consume_echo(&self, ev: &KeyEvent) -> bool {
        if ev.direction != KeyDirection::Press {
            return false;
        }
        // Where the gate can run, our emitter is unproxied — exactly
        // the condition under which the listener can tag our own
        // events. An untagged press there is the user's, and matching
        // it here would eat a real keystroke sharing a scancode with
        // something we just replayed.
        if self.key_gate.available() && !ev.injected {
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

    /// Is the user holding a modifier right now? Read from the last
    /// event seen, so it follows both presses and releases.
    pub(super) fn modifiers_held(&self) -> bool {
        let m = *self.held_modifiers.read();
        m.control || m.shift || m.alt || m.meta
    }

    pub(super) fn echo_pending(&self) -> bool {
        !self.expected_echo.lock().is_empty()
    }
}
