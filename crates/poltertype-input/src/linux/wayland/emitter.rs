//! `UinputEmitter` — replays corrections via a virtual keyboard.

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

pub struct UinputEmitter {
    device: parking_lot::Mutex<Option<VirtualDevice>>,
    /// Log of every key event actually written to uinput since the
    /// last [`KeyEmitter::take_emitted`]. Behind keyd (and similar
    /// remappers) our events echo back through the evdev listener
    /// with no `injected` marker; the engine uses this log to
    /// match-and-consume those echoes off the key stream.
    emitted: parking_lot::Mutex<Vec<EmittedKey>>,
}

impl UinputEmitter {
    pub fn new() -> Self {
        let s = Self {
            device: parking_lot::Mutex::new(None),
            emitted: parking_lot::Mutex::new(Vec::new()),
        };
        // Create the virtual keyboard eagerly. Input remappers (keyd
        // with `[ids] *`) grab every new keyboard asynchronously; if
        // the device springs into existence lazily at the FIRST
        // correction, that correction's opening backspaces race the
        // grab and some get lost on the floor — the user sees the
        // word's first letter survive. At startup the grab settles
        // long before any correction. Failure is fine (no permissions
        // yet); we retry lazily on first use.
        if let Err(e) = s.ensure_device() {
            warn!(?e, "uinput device creation deferred to first use");
        }
        s
    }

    fn ensure_device(&self) -> Result<(), InputError> {
        let mut g = self.device.lock();
        if g.is_some() {
            return Ok(());
        }
        let mut keys = AttributeSet::<KeyCode>::new();
        for code in 0u16..=255 {
            keys.insert(KeyCode::new(code));
        }
        // evdev 0.13 superseded `VirtualDeviceBuilder::new()` with
        // `VirtualDevice::builder()`.
        let dev = VirtualDevice::builder()
            .map_err(|e| InputError::Os(format!("uinput build: {e}")))?
            .name(EMITTER_DEVICE_NAME)
            .with_keys(&keys)
            .map_err(|e| InputError::Os(format!("uinput with_keys: {e}")))?
            .build()
            .map_err(|e| InputError::Os(format!("uinput create: {e}")))?;
        *g = Some(dev);
        Ok(())
    }
}

impl KeyEmitter for UinputEmitter {
    fn send_backspaces(&self, n: usize) -> Result<(), InputError> {
        if n == 0 {
            return Ok(());
        }
        self.ensure_device()?;
        let mut g = self.device.lock();
        let dev = g
            .as_mut()
            .ok_or_else(|| InputError::Os("uinput device not initialised".into()))?;
        // Same coalescing trap as `send_keys`: packing press + release
        // into one `emit` produces a single SYN_REPORT frame, and
        // libinput / keyd drop that as a zero-duration tap. The user
        // visible symptom was a backspace burst silently missing a
        // few presses, which left fragments of the previous word
        // (or its trailing space) on screen after a correction.
        let step = Duration::from_millis(4);
        for _ in 0..n {
            emit_one(
                dev,
                &self.emitted,
                InputEvent::new(EventType::KEY.0, KeyCode::KEY_BACKSPACE.0, 1),
            )?;
            thread::sleep(step);
            emit_one(
                dev,
                &self.emitted,
                InputEvent::new(EventType::KEY.0, KeyCode::KEY_BACKSPACE.0, 0),
            )?;
            thread::sleep(step);
        }
        Ok(())
    }

    fn send_keys(&self, keys: &[ReplayKey]) -> Result<(), InputError> {
        if keys.is_empty() {
            return Ok(());
        }
        debug!(count = keys.len(), "uinput replay starting");
        // No settle sleep here on purpose. `hyprctl switchxkblayout`
        // returns instantly while the compositor propagates the new
        // xkb state asynchronously, so a replay CAN outrun it (you see
        // the original `lfdfq` rather than `давай`) — but a blind
        // sleep at the last moment before emitting is precisely the
        // window in which a physical keystroke lands on screen ahead
        // of our text and scrambles the result. The engine owns that
        // wait now, measured from the actual layout switch and taken
        // before the deletion: see `LAYOUT_SETTLE` in poltertype-core.
        self.ensure_device()?;
        let mut g = self.device.lock();
        let dev = g
            .as_mut()
            .ok_or_else(|| InputError::Os("uinput device not initialised".into()))?;
        // `WordKey::scancode` is Win SC Set-1; on Linux those coincide
        // with evdev `KEY_*` codes for the alphanumeric / boundary rows
        // we ever buffer (see `evdev_to_sc1`). Anything outside that
        // band would have been filtered out by `WordBuffer::feed` long
        // before getting here.
        // Emit press / release as separate `dev.emit` calls. `emit`
        // packs everything into a single frame with one trailing
        // SYN_REPORT, which libinput treats as a "zero-duration tap"
        // and drops — the original missing-space symptom.
        //
        // We also pace the stream with a small inter-event delay.
        // Without it `keyd` (or any input remapper proxying our
        // uinput device) sees press/release pairs land within a few
        // microseconds of each other and silently coalesces or
        // discards the last one in a burst — most visibly the
        // trailing space after a corrected word. 4 ms per event is
        // well below human-noticeable for a 5-10 keystroke replay
        // and large enough to clear that coalescing window.
        let step = Duration::from_millis(4);
        let last_hold = Duration::from_millis(20);
        let boundary_guard = Duration::from_millis(12);
        let last_idx = keys.len() - 1;
        for (i, rk) in keys.iter().enumerate() {
            let kc = rk.scancode as u16;
            let is_last = i == last_idx;
            debug!(scancode = rk.scancode, shift = rk.shift, "uinput key");
            if rk.shift {
                emit_one(
                    dev,
                    &self.emitted,
                    InputEvent::new(EventType::KEY.0, KeyCode::KEY_LEFTSHIFT.0, 1),
                )?;
                thread::sleep(step);
            }
            // The very last key in the replay is the boundary the user
            // typed (almost always Space) — the key whose *press* just
            // triggered this correction. We react on that press within
            // ~10 ms, well before the user lifts their finger, so when
            // we reach this point the boundary key is still PHYSICALLY
            // HELD DOWN. Injecting a *press* for an already-down key is
            // a no-op at the compositor (global key state is already
            // "down"), so the boundary character never gets produced —
            // the corrected words run together with the space eaten,
            // exactly the long-standing "space gets cut" report.
            //
            // Fix: emit a release for the boundary scancode first, which
            // clears the held state regardless of whether the user is
            // still holding it (a harmless no-op if they already let
            // go). The following press is then a real down edge that
            // actually produces the character. The user's own later
            // release lands on an already-up key and is ignored.
            if is_last {
                emit_one(dev, &self.emitted, InputEvent::new(EventType::KEY.0, kc, 0))?;
                thread::sleep(boundary_guard);
            }
            emit_one(dev, &self.emitted, InputEvent::new(EventType::KEY.0, kc, 1))?;
            thread::sleep(if is_last { last_hold } else { step });
            emit_one(dev, &self.emitted, InputEvent::new(EventType::KEY.0, kc, 0))?;
            thread::sleep(if is_last { boundary_guard } else { step });
            if rk.shift {
                emit_one(
                    dev,
                    &self.emitted,
                    InputEvent::new(EventType::KEY.0, KeyCode::KEY_LEFTSHIFT.0, 0),
                )?;
                thread::sleep(step);
            }
        }
        Ok(())
    }

    fn release_modifiers(&self, held: Modifiers) -> Result<(), InputError> {
        let mut codes: Vec<KeyCode> = Vec::new();
        // Both sides of each: the listener tracks "shift is down", not
        // which shift, and releasing a key that is already up is a
        // no-op at the compositor.
        if held.control {
            codes.extend([KeyCode::KEY_LEFTCTRL, KeyCode::KEY_RIGHTCTRL]);
        }
        if held.shift {
            codes.extend([KeyCode::KEY_LEFTSHIFT, KeyCode::KEY_RIGHTSHIFT]);
        }
        if held.alt {
            codes.extend([KeyCode::KEY_LEFTALT, KeyCode::KEY_RIGHTALT]);
        }
        if held.meta {
            codes.extend([KeyCode::KEY_LEFTMETA, KeyCode::KEY_RIGHTMETA]);
        }
        if codes.is_empty() {
            return Ok(());
        }
        self.ensure_device()?;
        let mut g = self.device.lock();
        let dev = g
            .as_mut()
            .ok_or_else(|| InputError::Os("uinput device not initialised".into()))?;
        let step = Duration::from_millis(4);
        for kc in codes {
            release(dev, &self.emitted, kc)?;
            thread::sleep(step);
        }
        Ok(())
    }

    fn send_text(&self, text: &str) -> Result<(), InputError> {
        if text.is_empty() {
            return Ok(());
        }
        self.ensure_device()?;
        let mut g = self.device.lock();
        let dev = g
            .as_mut()
            .ok_or_else(|| InputError::Os("uinput device not initialised".into()))?;

        // Drive the GTK/Qt unicode-input combo: Ctrl+Shift+U <hex>
        // Space. This is the standard Linux-wide "type a Unicode
        // codepoint" sequence; it works in Firefox, Chromium, GTK and
        // Qt apps. Terminal emulators that disable it will need a
        // different path (Phase 6.x).
        for c in text.chars() {
            let cp = c as u32;
            let hex = format!("{cp:x}");
            // Ctrl+Shift+U
            press(dev, &self.emitted, KeyCode::KEY_LEFTCTRL)?;
            press(dev, &self.emitted, KeyCode::KEY_LEFTSHIFT)?;
            tap(dev, &self.emitted, KeyCode::KEY_U)?;
            release(dev, &self.emitted, KeyCode::KEY_LEFTSHIFT)?;
            release(dev, &self.emitted, KeyCode::KEY_LEFTCTRL)?;
            // Hex digits
            for ch in hex.chars() {
                if let Some(kc) = ascii_hex_to_keycode(ch) {
                    tap(dev, &self.emitted, kc)?;
                }
            }
            tap(dev, &self.emitted, KeyCode::KEY_SPACE)?;
        }
        Ok(())
    }

    fn take_emitted(&self) -> Vec<EmittedKey> {
        std::mem::take(&mut *self.emitted.lock())
    }

    fn backend_name(&self) -> &'static str {
        "linux-uinput"
    }
}
