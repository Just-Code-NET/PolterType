//! Key synthesis via `SendInput`.

use tracing::debug;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, SendInput, VIRTUAL_KEY, VK_BACK, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN,
    VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN,
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
        // Encode each char as UTF-16; non-BMP codepoints take two
        // INPUT events (high + low surrogate). Each codepoint is sent
        // both as a key-down and key-up.
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
        // not which shift, and `SendInput` of a key-up for a key that
        // is already up is a no-op.
        //
        // This clears the *logical* modifier state applications read
        // from the message queue, which is what decides whether our
        // replay arrives as text or as a burst of shortcuts. The user's
        // physical key stays down; their own release lands on an
        // already-up key and is ignored, and we deliberately do not
        // press the modifiers back — re-pressing one they have
        // meanwhile let go of would leave it stuck down.
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

    fn backend_name(&self) -> &'static str {
        "windows-sendinput"
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
