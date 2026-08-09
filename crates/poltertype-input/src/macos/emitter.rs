//! `CGEventPost` emitter.
//!
//! `CGEventKeyboardSetUnicodeString` gives macOS the same
//! layout-independent contract as Windows' `KEYEVENTF_UNICODE`, so
//! there is no scancode-replay path here — the trait default returns
//! `Unsupported` and the engine uses `send_text`.

use std::time::Duration;

use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGKeyCode};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use tracing::debug;

use super::codes::{
    FLAG_ALTERNATE, FLAG_COMMAND, FLAG_CONTROL, FLAG_SHIFT, KVK_COMMAND, KVK_CONTROL, KVK_DELETE,
    KVK_OPTION, KVK_SHIFT,
};
use super::consts::{EMITTER_TAG, K_CG_EVENT_SOURCE_USER_DATA};
use crate::{InputError, KeyEmitter, Modifiers};

/// Gap between the modifier releases and whatever we type next.
///
/// `CGEventPost` is asynchronous — it hands the event to the window
/// server and returns — so back-to-back posts can reach the focused
/// application in the same run-loop turn, before it has processed the
/// flags change. 4 ms matches the step the evdev and X11 emitters use
/// for the same reason.
const MODIFIER_SETTLE: Duration = Duration::from_millis(4);

pub struct MacosEmitter;

impl MacosEmitter {
    pub fn new() -> Self {
        Self
    }
}

fn event_source() -> Result<CGEventSource, InputError> {
    CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|()| InputError::Os("CGEventSource::new failed".into()))
}

/// Build one keyboard event, stamped as ours and stripped of modifiers.
///
/// Both halves matter. The stamp is what lets the listener tag the echo
/// `injected`. The stripping is the macOS half of
/// [`KeyEmitter::release_modifiers`]: an event from a `HIDSystemState`
/// source inherits the *current hardware* modifier flags, so with the
/// user still holding an accept chord our backspaces would post as ⌘⌫
/// and the replay as a burst of ⌃-shortcuts. Clearing the event's own
/// flags makes what the app receives independent of the user's fingers.
fn keyboard_event(
    src: &CGEventSource,
    keycode: CGKeyCode,
    key_down: bool,
) -> Result<CGEvent, InputError> {
    let ev = CGEvent::new_keyboard_event(src.clone(), keycode, key_down)
        .map_err(|()| InputError::Os("CGEvent::new_keyboard_event failed".into()))?;
    ev.set_flags(CGEventFlags::CGEventFlagNull);
    ev.set_integer_value_field(K_CG_EVENT_SOURCE_USER_DATA, EMITTER_TAG);
    Ok(ev)
}

impl KeyEmitter for MacosEmitter {
    fn send_backspaces(&self, n: usize) -> Result<(), InputError> {
        if n == 0 {
            return Ok(());
        }
        let src = event_source()?;
        for _ in 0..n {
            keyboard_event(&src, KVK_DELETE, true)?.post(CGEventTapLocation::HID);
            keyboard_event(&src, KVK_DELETE, false)?.post(CGEventTapLocation::HID);
        }
        Ok(())
    }

    fn send_text(&self, text: &str) -> Result<(), InputError> {
        if text.is_empty() {
            return Ok(());
        }
        let src = event_source()?;
        for c in text.chars() {
            let utf16: Vec<u16> = c.encode_utf16(&mut [0u16; 2]).to_vec();
            for key_down in [true, false] {
                let ev = keyboard_event(&src, 0, key_down)?;
                ev.set_string_from_utf16_unchecked(&utf16);
                ev.post(CGEventTapLocation::HID);
            }
        }
        Ok(())
    }

    fn release_modifiers(&self, held: Modifiers) -> Result<(), InputError> {
        // macOS has no key-up for a modifier: press and release are
        // both `kCGEventFlagsChanged` events whose *flags* say what is
        // down afterwards. So post one per modifier, each carrying the
        // picture that remains once that key is up, the last one empty.
        //
        // Left-hand keycodes only, like the X11 emitter: the
        // device-independent flag bits do not distinguish sides, and the
        // flags are what the receiving app reads.
        let mut remaining = 0u64;
        let mut releases: Vec<(CGKeyCode, u64)> = Vec::new();
        for (down, bit, keycode) in [
            (held.control, FLAG_CONTROL, KVK_CONTROL),
            (held.shift, FLAG_SHIFT, KVK_SHIFT),
            (held.alt, FLAG_ALTERNATE, KVK_OPTION),
            (held.meta, FLAG_COMMAND, KVK_COMMAND),
        ] {
            if down {
                remaining |= bit;
                releases.push((keycode, bit));
            }
        }
        if releases.is_empty() {
            return Ok(());
        }

        // Caps Lock is deliberately absent: it is a latch, not a held
        // key, and the engine folds it into `shift`. Posting a
        // flags-changed that clears it would turn the light off behind
        // the user's back.
        let src = event_source()?;
        for (keycode, bit) in releases {
            remaining &= !bit;
            let ev = CGEvent::new_keyboard_event(src.clone(), keycode, false)
                .map_err(|()| InputError::Os("CGEvent::new_keyboard_event failed".into()))?;
            ev.set_type(CGEventType::FlagsChanged);
            ev.set_flags(CGEventFlags::from_bits_truncate(remaining));
            ev.set_integer_value_field(K_CG_EVENT_SOURCE_USER_DATA, EMITTER_TAG);
            ev.post(CGEventTapLocation::HID);
            std::thread::sleep(MODIFIER_SETTLE);
        }
        debug!(?held, "posted macOS modifier releases");
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "macos-cg-event-post"
    }
}
