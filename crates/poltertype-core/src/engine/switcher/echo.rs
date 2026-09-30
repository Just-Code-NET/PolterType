//! Match-and-consume tracking of our own injected keystrokes echoing
//! back through the listener (Linux behind keyd & friends). See the
//! `expected_echo` field docs for the full rationale.

use std::time::{Duration, Instant};

use poltertype_input::{EmittedKey, KeyDirection, KeyEvent};

use crate::engine::consts::KEY_DOWN_STALE;
use crate::engine::heuristics::modifier_role;

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

    /// Follow which non-modifier keys are physically down. Fed every
    /// user event the engine reads, on either path to it — the run loop
    /// and a correction draining the stream itself.
    pub(super) fn track_keys_down(&self, ev: &KeyEvent) {
        if ev.injected
            || modifier_role(ev.scancode).is_some()
            || ev.scancode == poltertype_types::SC_POINTER_BUTTON
        {
            return;
        }
        let mut down = self.keys_down.lock();
        down.retain(|&(sc, _)| sc != ev.scancode);
        if ev.direction == KeyDirection::Press {
            down.push((ev.scancode, Instant::now()));
        }
    }

    /// Is the user still holding a key a correction must not emit
    /// under? Most often the boundary that triggered it: we act on its
    /// press, and the finger comes up a moment later.
    ///
    /// Emitting while it is down loses that release on Linux whenever
    /// the key gate holds the keyboard — the compositor saw the press
    /// and never sees the release, so libinput keeps the key down, drops
    /// our replayed press of it and the user's next one too, and the
    /// space takes three presses to appear (issue #74).
    pub(super) fn keys_held(&self) -> bool {
        self.keys_down
            .lock()
            .iter()
            .any(|&(_, at)| at.elapsed() < KEY_DOWN_STALE)
    }

    /// Is the user holding a modifier right now? Read from the last
    /// event seen, so it follows both presses and releases.
    pub(super) fn modifiers_held(&self) -> bool {
        let m = *self.held_modifiers.read();
        m.control || m.shift || m.alt || m.meta
    }

    /// Is Caps Lock latched right now? Only the paths that turn
    /// *characters* into keystrokes need it — a scancode replay carries
    /// its own Shift state and the lock applies itself.
    pub(super) fn caps_on(&self) -> bool {
        self.held_modifiers.read().caps
    }

    pub(super) fn echo_pending(&self) -> bool {
        !self.expected_echo.lock().is_empty()
    }
}
