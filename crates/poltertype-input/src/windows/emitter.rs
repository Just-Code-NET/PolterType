//! Key synthesis via `SendInput`.

use tracing::debug;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP,
    KEYEVENTF_SCANCODE, KEYEVENTF_UNICODE, SendInput, VIRTUAL_KEY, VK_BACK, VK_LCONTROL, VK_LMENU,
    VK_LSHIFT, VK_LWIN, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN,
};

use super::consts::EMITTER_MARKER;
use crate::{InputError, KeyEmitter, Modifiers};

// ─── KeyEmitter (SendInput) ──────────────────────────────────────────

pub struct WindowsEmitter;

impl WindowsEmitter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WindowsEmitter {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyEmitter for WindowsEmitter {
    fn send_backspaces(&self, n: usize) -> Result<(), InputError> {
        if n == 0 {
            return Ok(());
        }
        let mut events: Vec<INPUT> = Vec::with_capacity(n * 2);
        for _ in 0..n {
            events.push(make_vk_input(VK_BACK, false));
            events.push(make_vk_input(VK_BACK, true));
        }
        send_inputs(&events)
    }

    fn send_text(&self, text: &str) -> Result<(), InputError> {
        if text.is_empty() {
            return Ok(());
        }
        let mut events: Vec<INPUT> = Vec::with_capacity(text.len() * 2);
        for c in text.chars() {
            let mut buf = [0u16; 2];
            for &unit in c.encode_utf16(&mut buf).iter() {
                events.push(make_unicode_input(unit, false));
                events.push(make_unicode_input(unit, true));
            }
        }
        send_inputs(&events)
    }

    fn release_modifiers(&self, held: Modifiers) -> Result<(), InputError> {
        // Both sides of each: `read_modifiers` reports "shift is down",
        // not which shift, and a key-up for an already-up key is a
        // no-op.
        //
        // This clears the *logical* modifier state applications read
        // from the message queue, which decides whether our replay
        // arrives as text or as shortcuts. The modifiers are
        // deliberately not pressed back — re-pressing one the user has
        // meanwhile released would leave it stuck down.
        let mut events: Vec<INPUT> = Vec::new();
        for (down, keys) in [
            (held.control, [VK_LCONTROL, VK_RCONTROL].as_slice()),
            (held.shift, [VK_LSHIFT, VK_RSHIFT].as_slice()),
            (held.alt, [VK_LMENU, VK_RMENU].as_slice()),
            (held.meta, [VK_LWIN, VK_RWIN].as_slice()),
        ] {
            if down {
                events.extend(keys.iter().map(|&vk| make_vk_input(vk, true)));
            }
        }
        if events.is_empty() {
            return Ok(());
        }
        debug!(?held, "releasing held modifiers before emitting");
        send_inputs(&events)
    }

    /// Hold modifiers around one key — what selection conversion needs
    /// to press `Ctrl+C` into the focused application (issue #32).
    ///
    /// By scancode, not by virtual key: the engine reasons in Set-1
    /// scancodes throughout, and `KEYEVENTF_SCANCODE` is what makes the
    /// press independent of whatever layout the user is in — `C` is not
    /// on the same virtual key everywhere.
    fn send_chord(&self, chord: poltertype_types::SwitchChord) -> Result<(), InputError> {
        let mut mods: Vec<VIRTUAL_KEY> = Vec::new();
        if chord.ctrl {
            mods.push(VK_LCONTROL);
        }
        if chord.shift {
            mods.push(VK_LSHIFT);
        }
        if chord.alt {
            mods.push(VK_LMENU);
        }
        if chord.meta {
            mods.push(VK_LWIN);
        }
        let mut events: Vec<INPUT> = Vec::new();
        events.extend(mods.iter().map(|&vk| make_vk_input(vk, false)));
        // A bare-modifier chord carries no key of its own; the second
        // modifier is the key. Same shape as the Linux emitter.
        if chord.scancode != 0 {
            events.push(make_scancode_input(chord.scancode as u16, false));
            events.push(make_scancode_input(chord.scancode as u16, true));
        }
        events.extend(mods.iter().rev().map(|&vk| make_vk_input(vk, true)));
        send_inputs(&events)
    }

    fn backend_name(&self) -> &'static str {
        "windows-sendinput"
    }
}

/// One key press or release addressed by Set-1 scancode.
fn make_scancode_input(scancode: u16, key_up: bool) -> INPUT {
    let mut flags = KEYEVENTF_SCANCODE;
    if key_up {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: scancode,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn make_vk_input(vk: VIRTUAL_KEY, key_up: bool) -> INPUT {
    let flags = if key_up {
        KEYEVENTF_KEYUP
    } else {
        KEYBD_EVENT_FLAGS(0)
    };
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                // The gate reads this back: an event without the
                // marker is somebody else's and gets held.
                dwExtraInfo: EMITTER_MARKER,
            },
        },
    }
}

fn make_unicode_input(unit: u16, key_up: bool) -> INPUT {
    let mut flags = KEYEVENTF_UNICODE;
    if key_up {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: unit,
                dwFlags: flags,
                time: 0,
                // The gate reads this back: an event without the
                // marker is somebody else's and gets held.
                dwExtraInfo: EMITTER_MARKER,
            },
        },
    }
}

fn send_inputs(events: &[INPUT]) -> Result<(), InputError> {
    if events.is_empty() {
        return Ok(());
    }
    // Safety: SendInput requires a contiguous INPUT slice; we pass
    // exactly that. Returns the number actually inserted.
    let n = unsafe { SendInput(events, std::mem::size_of::<INPUT>() as i32) };
    if n as usize != events.len() {
        return Err(InputError::Os(format!(
            "SendInput sent {n}/{} events",
            events.len()
        )));
    }
    Ok(())
}
