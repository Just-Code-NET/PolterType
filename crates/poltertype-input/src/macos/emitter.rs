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
    KVK_OPTION, KVK_SHIFT, sc1_to_mac_keycode,
};
use super::consts::{EMITTER_TAG, K_CG_EVENT_SOURCE_USER_DATA};
use crate::{InputError, KeyEmitter, Modifiers, SwitchChord};

/// Gap between the modifier releases and whatever we type next.
///
/// `CGEventPost` is asynchronous — it hands the event to the window
/// server and returns — so back-to-back posts can reach the focused
/// application in the same run-loop turn, before it has processed the
/// flags change. 4 ms matches the step the evdev and X11 emitters use
/// for the same reason.
const MODIFIER_SETTLE: Duration = Duration::from_millis(4);

/// Gap between individual key events inside one burst.
///
/// The hazard [`MODIFIER_SETTLE`] describes applies to key events too,
/// and harder: a backspace burst posted
/// back-to-back reaches the focused app within a single run-loop
/// turn, and AppKit coalesces same-key down/up pairs that share a
/// timestamp — observably, one delete in six was landing, which left
/// the mistyped word's first letter on screen ahead of the
/// replacement (and one *extra* landing delete ate the separator
/// before the word). 2 ms matches the X11 emitter's `KEY_STEP`.
const KEY_STEP: Duration = Duration::from_millis(2);

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
            std::thread::sleep(KEY_STEP);
            keyboard_event(&src, KVK_DELETE, false)?.post(CGEventTapLocation::HID);
            std::thread::sleep(KEY_STEP);
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
                std::thread::sleep(KEY_STEP);
            }
        }
        Ok(())
    }

    fn send_chord(&self, chord: SwitchChord) -> Result<(), InputError> {
        // The chord arrives in SC-1 space, like everything the engine
        // says; the reverse table answers `None` for a key it cannot
        // name, and pressing a *wrong* key with Cmd held is a real
        // action in someone's application — so refuse, loudly.
        let Some(kvk) = sc1_to_mac_keycode(chord.scancode) else {
            return Err(InputError::Unsupported(format!(
                "no Apple keycode for SC-1 {:#04x}",
                chord.scancode
            )));
        };

        let mut flags = 0u64;
        for (on, bit) in [
            (chord.ctrl, FLAG_CONTROL),
            (chord.shift, FLAG_SHIFT),
            (chord.alt, FLAG_ALTERNATE),
            (chord.meta, FLAG_COMMAND),
        ] {
            if on {
                flags |= bit;
            }
        }

        // The modifiers travel as flags *on the key events* rather than
        // as their own press/release pair. That is how macOS itself
        // matches menu shortcuts — the receiver reads the event's
        // flags, not the modifier keys' physical state — and it leaves
        // nothing to get stuck: no down was posted, so no up is owed.
        // `keyboard_event` has already stripped the hardware flags, so
        // what the app sees is exactly the chord and not the chord plus
        // whatever the user's fingers are doing.
        let src = event_source()?;
        for key_down in [true, false] {
            let ev = keyboard_event(&src, CGKeyCode::from(kvk), key_down)?;
            ev.set_flags(CGEventFlags::from_bits_truncate(flags));
            ev.post(CGEventTapLocation::HID);
            std::thread::sleep(KEY_STEP);
        }
        debug!(scancode = chord.scancode, kvk, flags, "posted macOS chord");
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
