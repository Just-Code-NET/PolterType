//! Hotkeys matched off the key stream, the suggestion-accept digit
//! chords (every platform), and smart-command dispatch.

use std::sync::Arc;
use std::time::Instant;

use crossbeam_channel::Receiver;
use poltertype_input::{KeyDirection, KeyEvent};
use tracing::{info, warn};

use crate::commands::{CommandAction, UserCommand};
use crate::engine::buffer::WordBuffer;
use crate::engine::enums::EngineCommand;
use crate::engine::heuristics::match_binding;
use crate::engine::types::{Binding, BindingState};

use super::engine::SwitcherEngine;

impl SwitcherEngine {
    /// Match the raw key event against whatever the app asked us to
    /// watch for: an ordinary chord where the OS grab is deaf (the
    /// Wayland/evdev backend), and a modifier-only chord anywhere,
    /// since that one has no key code to register.
    ///
    /// Runs before the paused early-return in `handle_key`, so the pause
    /// chord can also *resume*. Our own replayed corrections cannot
    /// re-trigger a chord: `injected` events are ignored, and untagged
    /// echoes were already consumed by `consume_echo`.
    pub(super) fn check_keystream_hotkeys(
        &self,
        ev: &KeyEvent,
        buffer: &mut WordBuffer,
        key_rx: &Receiver<KeyEvent>,
    ) {
        if ev.injected {
            return;
        }
        let hk = *self.keystream_hotkeys.read();
        // One clock read for both, and only where a binding exists:
        // this runs on every key event on every backend.
        let now = (hk.pause.is_some() || hk.switch_last.is_some()).then(Instant::now);
        // Matched under the lock, dispatched outside it: a command can
        // run a whole correction, whose window observes releases
        // through this same state.
        let (pause, switch) = {
            let mut st = self.chord_state.lock();
            let fire = |b: Option<Binding>, s: &mut BindingState| match (b, now) {
                (Some(b), Some(now)) => match_binding(ev, b, s, now),
                _ => false,
            };
            let pause = fire(hk.pause, &mut st.pause);
            (pause, fire(hk.switch_last, &mut st.switch))
        };
        if pause {
            self.handle_command(EngineCommand::TogglePause, buffer, key_rx);
        }
        if switch {
            self.handle_command(EngineCommand::SwitchLastForcefully, buffer, key_rx);
        }
        self.check_suggestion_chord(ev, buffer, key_rx);
    }

    /// Is this press the force-switch (or pause) chord itself,
    /// repeating?
    ///
    /// evdev reports a held key as repeated presses, so a chord kept
    /// down past the kernel's repeat delay keeps arriving while the
    /// correction it asked for is still being emitted. The correction
    /// window reads any press carrying Ctrl/Alt/Meta as a shortcut it
    /// cannot reconstruct and abandons the whole correction — so
    /// holding the hotkey a moment too long did nothing at all, or
    /// worse, since the abandon also drops the stash and taints the
    /// buffer (issue #39).
    ///
    /// Both the chords matched here and the ones an OS-level grab
    /// delivers: a grab does not stop the key reaching our listener,
    /// only our matcher, and X11 in particular keeps the keyboard
    /// grabbed for as long as the key is down — so the repeats arrive
    /// exactly where they do the most damage.
    pub(super) fn is_own_hotkey_press(&self, ev: &KeyEvent) -> bool {
        self.keystream_hotkeys.read().chords().any(|c| {
            ev.scancode == c.scancode
                && ev.modifiers.control == c.ctrl
                && ev.modifiers.shift == c.shift
                && ev.modifiers.alt == c.alt
                && ev.modifiers.meta == c.meta
        })
    }

    /// Is the gesture that asked for this correction still down?
    ///
    /// Deliberately not "is any modifier held": a word closed by a
    /// shifted separator is corrected with Shift still down, and making
    /// *that* wait is a two-second stall on ordinary typing. The
    /// question is whether what is held is a hotkey's own modifier set
    /// — as close as an OS-level grab lets us get, since nothing tells
    /// the engine which key that grab delivered. Where we match the
    /// chord ourselves the latch is exact, and it is what answers for a
    /// binding with no modifier in it at all.
    pub(super) fn trigger_held(&self) -> bool {
        let m = *self.held_modifiers.read();
        if !(m.control || m.shift || m.alt || m.meta) {
            let st = self.chord_state.lock();
            return st.switch.key_down || st.pause.key_down;
        }
        self.keystream_hotkeys.read().chords().any(|c| {
            m.control == c.ctrl && m.shift == c.shift && m.alt == c.alt && m.meta == c.meta
        })
    }

    /// Keep the chord latches honest about keys the correction window
    /// swallowed.
    ///
    /// Every matcher here is edge-triggered: one fire per physical
    /// press, latched until the release. But a correction reads key
    /// events straight off the channel, so a release landing inside one
    /// never reaches [`Self::check_keystream_hotkeys`] and the latch
    /// stays down for good — the force-switch then answers every
    /// *other* press, and the default `Ctrl+Shift+Space` pause chord
    /// dies outright at the first correction a Space ever triggers,
    /// since that Space's own release is the one swallowed.
    ///
    /// Releases only, and whatever they match is dropped rather than
    /// dispatched: we are inside `apply_correction` and must not
    /// re-enter it.
    pub(super) fn observe_swallowed_release(&self, ev: &KeyEvent) {
        if ev.injected || ev.direction != KeyDirection::Release {
            return;
        }
        let hk = *self.keystream_hotkeys.read();
        let now = Instant::now();
        let mut st = self.chord_state.lock();
        if let Some(b) = hk.pause {
            let _ = match_binding(ev, b, &mut st.pause, now);
        }
        if let Some(b) = hk.switch_last {
            let _ = match_binding(ev, b, &mut st.switch, now);
        }
        if let Some(i) = suggestion_digit_index(ev.scancode) {
            st.suggest_digit_down[i] = false;
        }
    }

    /// The suggestion-accept digit chord (`<modifiers>+1` … `+9`).
    ///
    /// Runs on *every* backend, not just Wayland: registering nine
    /// OS-level global hotkeys would steal those combos from every
    /// application even with no tooltip up. The trade-off is that the
    /// keypress still reaches the focused app, which is why the default
    /// chord is Ctrl+Shift+digit.
    fn check_suggestion_chord(
        &self,
        ev: &KeyEvent,
        buffer: &mut WordBuffer,
        key_rx: &Receiver<KeyEvent>,
    ) {
        let Some(index) = suggestion_digit_index(ev.scancode) else {
            return;
        };
        match ev.direction {
            KeyDirection::Release => {
                self.chord_state.lock().suggest_digit_down[index] = false;
            }
            KeyDirection::Press => {
                {
                    let latched = &mut self.chord_state.lock().suggest_digit_down[index];
                    if *latched {
                        return; // autorepeat
                    }
                    *latched = true;
                }
                let generation = {
                    let slot = self.pending_suggestion.lock();
                    slot.as_ref().and_then(|p| {
                        let a = p.accept?;
                        (ev.modifiers.control == a.ctrl
                            && ev.modifiers.shift == a.shift
                            && ev.modifiers.alt == a.alt
                            && ev.modifiers.meta == a.meta
                            && index < p.entries.len())
                        .then_some(p.generation)
                    })
                };
                if let Some(generation) = generation {
                    self.handle_command(
                        EngineCommand::AcceptSuggestion {
                            generation,
                            index,
                            typed_digit: true,
                            from_pointer: false,
                        },
                        buffer,
                        key_rx,
                    );
                }
            }
        }
    }

    /// Run a matched smart command: backspace the typed trigger plus the
    /// boundary character, then dispatch the action.
    ///
    /// `backspace_count` is in characters, not bytes — what
    /// `emit_backspace` uses across multibyte triggers.
    ///
    /// `TypeText` re-emits the boundary after the expansion so the
    /// user's flow continues. `SwitchLayout` and `OpenPath` keep it
    /// consumed: the user wanted a side effect, not text.
    pub(super) fn dispatch_smart_command(
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
        let sent = self.key_emitter.send_backspaces(backspace_count);
        self.push_echoes(self.key_emitter.take_emitted());
        if let Err(e) = sent {
            warn!(?e, id = %cmd.id, "smart command: send_backspaces failed");
            return;
        }
        match &cmd.action {
            CommandAction::TypeText { text } => {
                let sent = self.key_emitter.send_text(text);
                self.push_echoes(self.key_emitter.take_emitted());
                if let Err(e) = sent {
                    warn!(?e, id = %cmd.id, "smart command: send_text failed");
                    return;
                }
                let mut buf = [0u8; 4];
                let s = boundary_char.encode_utf8(&mut buf);
                let sent = self.key_emitter.send_text(s);
                self.push_echoes(self.key_emitter.take_emitted());
                if let Err(e) = sent {
                    warn!(?e, id = %cmd.id, "smart command: re-emit boundary failed");
                }
            }
            CommandAction::SwitchLayout { layout } => {
                // Backspaces have already gone out, so an impossible
                // switch cannot be recovered from — log loudly.
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
            CommandAction::RunShell(shell) => self.dispatch_run_shell(cmd, shell, boundary_char),
        }
    }

    /// Run a `run_shell` command off the correction path.
    ///
    /// The word-boundary handler stands between the user's keystroke and
    /// the corrected word and must return promptly, while a command can
    /// block for up to `shell::RUN_TIMEOUT` — hence the worker thread.
    ///
    /// The refusal check happens here as well as at settings load:
    /// `allow_run_shell` can be turned off while the app runs, and an
    /// entry legal at startup must stop working the moment it is.
    fn dispatch_run_shell(
        &self,
        cmd: &UserCommand,
        shell: &crate::commands::ShellCommand,
        boundary_char: char,
    ) {
        let allow = self.settings.snapshot().commands_allow_run_shell;
        if let Err(refusal) = crate::commands::check(shell, allow) {
            warn!(id = %cmd.id, %refusal, "smart command: refused");
            return;
        }

        let shell = shell.clone();
        let id = cmd.id.clone();
        let emitter = Arc::clone(&self.key_emitter);
        let spawned = std::thread::Builder::new()
            .name("poltertype-smart-command".into())
            .spawn(move || {
                let Some(output) = crate::commands::run(&shell) else {
                    return;
                };
                // Safe off-thread for the same reason the correction
                // replay is: the emitter is `Send + Sync` and every
                // emitted key comes back through the listener as ours.
                let mut text = output;
                text.push(boundary_char);
                if let Err(e) = emitter.send_text(&text) {
                    warn!(?e, %id, "smart command: typing output failed");
                }
            });
        if let Err(e) = spawned {
            warn!(%e, id = %cmd.id, "smart command: could not start worker thread");
        }
    }
}

/// Which suggestion-accept digit a scancode is, if any: the digit row
/// `1`..=`9` (SC Set-1 `0x02`..=`0x0A`).
fn suggestion_digit_index(scancode: u32) -> Option<usize> {
    (0x02..=0x0A)
        .contains(&scancode)
        .then(|| (scancode - 0x02) as usize)
}
