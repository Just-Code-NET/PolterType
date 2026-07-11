//! Keystream hotkey chords (Wayland path) and smart-command dispatch.

use crossbeam_channel::Receiver;
use poltertype_input::KeyEvent;
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
        }
    }
}
