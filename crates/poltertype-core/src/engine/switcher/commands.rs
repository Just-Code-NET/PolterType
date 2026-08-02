//! Keystream hotkey chords (Wayland path), the suggestion-accept
//! digit chords (every platform), and smart-command dispatch.

use std::sync::Arc;

use crossbeam_channel::Receiver;
use poltertype_input::{KeyDirection, KeyEvent};
use tracing::{info, warn};

use crate::commands::{CommandAction, UserCommand};
use crate::engine::buffer::WordBuffer;
use crate::engine::enums::EngineCommand;
use crate::engine::heuristics::match_chord;
use crate::engine::types::ChordState;

use super::engine::SwitcherEngine;

impl SwitcherEngine {
    /// Match the raw key event against the keystream hotkeys (Wayland
    /// path). Mirrors what the OS `global-hotkey` grab does on other
    /// backends, dispatching the same [`EngineCommand`]s.
    ///
    /// Our own replayed corrections can't re-trigger a chord here:
    /// `injected` events are ignored outright, and untagged echoes
    /// (keyd & friends) were already consumed by `consume_echo` in the
    /// run loop before this is called — during a manual switch the
    /// user may still be holding `Ctrl+Shift` while our uinput
    /// backspaces echo back as `Ctrl+Shift+Backspace`. Run before
    /// the paused early-return in `handle_key` so the pause chord can
    /// also *resume*.
    pub(super) fn check_keystream_hotkeys(
        &self,
        ev: &KeyEvent,
        state: &mut ChordState,
        buffer: &mut WordBuffer,
        key_rx: &Receiver<KeyEvent>,
    ) {
        if ev.injected {
            return;
        }
        let hk = *self.keystream_hotkeys.read();
        if let Some(c) = hk.pause {
            if match_chord(ev, c, &mut state.pause_key_down) {
                self.handle_command(EngineCommand::TogglePause, buffer, key_rx);
            }
        }
        if let Some(c) = hk.switch_last {
            if match_chord(ev, c, &mut state.switch_key_down) {
                self.handle_command(EngineCommand::SwitchLastForcefully, buffer, key_rx);
            }
        }
        self.check_suggestion_chord(ev, state, buffer, key_rx);
    }

    /// The suggestion-accept digit chord (`<modifiers>+1` … `+9`).
    ///
    /// Unlike the two chords above, this runs on *every* backend, not
    /// just Wayland: registering nine OS-level global hotkeys
    /// permanently would steal those combos from every application
    /// even while no tooltip is up, whereas stream matching costs one
    /// mutex peek per digit keypress and only while an offer is
    /// pending. The trade-off (shared with Wayland hotkeys): the
    /// keypress still reaches the focused app — which is why the
    /// default chord is Ctrl+Shift+digit, a combination virtually no
    /// application binds.
    fn check_suggestion_chord(
        &self,
        ev: &KeyEvent,
        state: &mut ChordState,
        buffer: &mut WordBuffer,
        key_rx: &Receiver<KeyEvent>,
    ) {
        // Digit row 1..=9 (SC Set-1 0x02..=0x0A).
        let Some(index) = (0x02..=0x0A)
            .contains(&ev.scancode)
            .then(|| (ev.scancode - 0x02) as usize)
        else {
            return;
        };
        let latched = &mut state.suggest_digit_down[index];
        match ev.direction {
            KeyDirection::Release => {
                *latched = false;
            }
            KeyDirection::Press => {
                if *latched {
                    return; // autorepeat
                }
                *latched = true;
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

    /// Run a matched smart command. The shape is the same for every
    /// action variant: backspace the typed trigger plus the
    /// boundary character (so the magic word disappears from the
    /// user's text), then dispatch the action.
    ///
    /// `backspace_count` is precomputed by the caller as
    /// `current_text.chars().count() + 1` — counting characters not
    /// bytes, because that's what the OS-level emit_backspace uses
    /// across Cyrillic / multibyte triggers.
    ///
    /// For `TypeText` we re-emit the boundary character after the
    /// expansion so the user's flow continues naturally — typing
    /// `anrl<space>` ends up with the cursor after a trailing space
    /// in the expansion, not glued to the last word. For
    /// `SwitchLayout` and `OpenPath` the boundary stays consumed
    /// (the user wanted a side-effect, not text).
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
                // Re-emit the boundary so the user's typing flow
                // continues — they typed `anrl<space>`, they expect
                // `<expansion><space>` afterward, not the cursor
                // glued to the end.
                let mut buf = [0u8; 4];
                let s = boundary_char.encode_utf8(&mut buf);
                let sent = self.key_emitter.send_text(s);
                self.push_echoes(self.key_emitter.take_emitted());
                if let Err(e) = sent {
                    warn!(?e, id = %cmd.id, "smart command: re-emit boundary failed");
                }
            }
            CommandAction::SwitchLayout { layout } => {
                // Same pre-flight as `apply_correction`: confirm the
                // layout is reachable before doing anything else.
                // Backspaces have already happened — if the switch
                // is impossible we can't recover the deleted text,
                // but we log loudly so the user notices.
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
    /// The word-boundary handler must return promptly — it is what
    /// stands between the user's keystroke and the corrected word —
    /// and a user command can block for up to `shell::RUN_TIMEOUT`.
    /// So the process is started on a worker thread, and the thread
    /// types the output when there is any.
    ///
    /// The refusal check happens here as well as at settings load:
    /// `allow_run_shell` can be turned off while the app runs, and
    /// the entry that was legal at startup must stop working the
    /// moment it is.
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
                // Typing from a worker thread is safe for the same
                // reason the correction replay is: the emitter is
                // `Send + Sync` and every emitted key comes back
                // through the listener marked as ours.
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
