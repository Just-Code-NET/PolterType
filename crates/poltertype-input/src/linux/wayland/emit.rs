//! Low-level key emission helpers.

use super::*;
use crate::{
    EmittedKey, InputError, InputListener, KeyDirection, KeyEmitter, KeyEvent, Modifiers, ReplayKey,
};
use crossbeam_channel::Sender;
use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, Device, EventType, InputEvent, KeyCode};
use poltertype_types::SC_POINTER_BUTTON;
use std::collections::HashSet;
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use tracing::{debug, info, trace, warn};

/// Emit a single key event and, on success, record it in the emitter's
/// echo log (see [`UinputEmitter::emitted`]). Recording after the
/// write means a failed emit never leaves a phantom entry that would
/// eat a real user keystroke later.
pub(crate) fn emit_one(
    dev: &mut VirtualDevice,
    log: &parking_lot::Mutex<Vec<EmittedKey>>,
    ev: InputEvent,
) -> Result<(), InputError> {
    dev.emit(&[ev])
        .map_err(|e| InputError::Os(format!("uinput emit: {e}")))?;
    log.lock().push(EmittedKey {
        scancode: ev.code() as u32,
        direction: if ev.value() == 0 {
            KeyDirection::Release
        } else {
            KeyDirection::Press
        },
    });
    Ok(())
}

pub(crate) fn press(
    dev: &mut VirtualDevice,
    log: &parking_lot::Mutex<Vec<EmittedKey>>,
    k: KeyCode,
) -> Result<(), InputError> {
    emit_one(dev, log, InputEvent::new(EventType::KEY.0, k.0, 1))
}

pub(crate) fn release(
    dev: &mut VirtualDevice,
    log: &parking_lot::Mutex<Vec<EmittedKey>>,
    k: KeyCode,
) -> Result<(), InputError> {
    emit_one(dev, log, InputEvent::new(EventType::KEY.0, k.0, 0))
}

pub(crate) fn tap(
    dev: &mut VirtualDevice,
    log: &parking_lot::Mutex<Vec<EmittedKey>>,
    k: KeyCode,
) -> Result<(), InputError> {
    // Two separate emits on purpose — a single frame with both edges
    // is a "zero-duration tap" that libinput / keyd silently drop.
    press(dev, log, k)?;
    release(dev, log, k)
}

pub(crate) fn ascii_hex_to_keycode(c: char) -> Option<KeyCode> {
    Some(match c {
        '0' => KeyCode::KEY_0,
        '1' => KeyCode::KEY_1,
        '2' => KeyCode::KEY_2,
        '3' => KeyCode::KEY_3,
        '4' => KeyCode::KEY_4,
        '5' => KeyCode::KEY_5,
        '6' => KeyCode::KEY_6,
        '7' => KeyCode::KEY_7,
        '8' => KeyCode::KEY_8,
        '9' => KeyCode::KEY_9,
        'a' | 'A' => KeyCode::KEY_A,
        'b' | 'B' => KeyCode::KEY_B,
        'c' | 'C' => KeyCode::KEY_C,
        'd' | 'D' => KeyCode::KEY_D,
        'e' | 'E' => KeyCode::KEY_E,
        'f' | 'F' => KeyCode::KEY_F,
        _ => return None,
    })
}

/// Linux evdev keycode → Win SC Set-1 scancode.
///
/// For the alphanumeric / number / boundary rows we care about,
/// Linux's evdev `KEY_*` codes coincide with SC Set-1 (e.g. evdev
/// 1 = Esc = SC1 0x01, evdev 30 = A = SC1 0x1E). Extended keys
/// (arrows, NumLock area) diverge — they're discarded by the
/// engine's word-buffer classifier so we don't bother re-mapping
/// them yet. Phase 6.x can add the table when X11 fallback lands.
///
/// Reference: https://www.kernel.org/doc/Documentation/input/event-codes.txt
pub(crate) fn evdev_to_sc1(evdev: u32) -> u32 {
    evdev
}
