//! `UinputEmitter` — replays corrections via a virtual keyboard.

use super::*;
use crate::{
    EmittedKey, InputError, InputListener, KeyDirection, KeyEmitter, KeyEvent, Modifiers,
    ReplayKey, SwitchChord,
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
        // Eagerly, because input remappers (keyd with `[ids] *`) grab
        // every new keyboard asynchronously. A device created lazily at
        // the first correction has its opening backspaces race that
        // grab, and the word's first letter survives. Failure here is
        // fine — no permissions yet; we retry on first use.
        if let Err(e) = s.ensure_device() {
            warn!(?e, "uinput device creation deferred to first use");
        }
        s
    }

    /// Whether the virtual keyboard actually exists. `new` creates it
    /// eagerly and tolerates failure, so this is how a caller tells
    /// "ready" from "will retry and probably fail again" — which is
    /// what decides whether the portal is worth asking about.
    pub fn is_usable(&self) -> bool {
        if self.device.lock().is_some() {
            return true;
        }
        // One retry: the eager attempt may have run before udev
        // applied the rule that grants access.
        self.ensure_device().is_ok()
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
        let mut dev = VirtualDevice::builder()
            .map_err(|e| InputError::Os(format!("uinput build: {e}")))?
            .name(EMITTER_DEVICE_NAME)
            .with_keys(&keys)
            .map_err(|e| InputError::Os(format!("uinput with_keys: {e}")))?
            .build()
            .map_err(|e| InputError::Os(format!("uinput create: {e}")))?;
        // Record the node(s) the kernel assigned, so the gate can
        // exclude our own device by identity rather than trusting the
        // name comparison alone — see `own_nodes` for why that
        // matters.
        match dev.enumerate_dev_nodes_blocking() {
            Ok(nodes) => {
                for node in nodes.flatten() {
                    debug!(?node, "uinput emitter node recorded");
                    own_nodes::record(node);
                }
            }
            Err(e) => warn!(
                ?e,
                "could not enumerate our uinput node — device discovery falls back to name matching"
            ),
        }
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
        // Same coalescing trap as `send_keys`: press + release in one
        // `emit` is a single SYN_REPORT frame, which libinput/keyd drop
        // as a zero-duration tap. Symptom was a backspace burst missing
        // presses, leaving fragments of the previous word on screen.
        // Logged like the replay is: a burst that goes out and erases
        // nothing looks exactly like a burst that was never sent, and
        // telling the two apart took a day without this line.
        debug!(count = n, "uinput backspaces starting");
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
        // No settle sleep here on purpose. A blind sleep immediately
        // before emitting is precisely the window in which a physical
        // keystroke lands ahead of our text and scrambles it. The engine
        // owns that wait, measured from the actual layout switch and
        // taken before the deletion — see `LAYOUT_SETTLE`.
        self.ensure_device()?;
        let mut g = self.device.lock();
        let dev = g
            .as_mut()
            .ok_or_else(|| InputError::Os("uinput device not initialised".into()))?;
        // `WordKey::scancode` is Win SC Set-1, which coincides with the
        // evdev `KEY_*` codes for every row we buffer; anything else was
        // filtered by `WordBuffer::feed` long before this.
        //
        // Press and release must be separate `dev.emit` calls: one call
        // packs them into a single frame with one SYN_REPORT, which
        // libinput treats as a zero-duration tap and drops.
        //
        // The 4 ms pacing is for remappers proxying our uinput device —
        // keyd coalesces or discards pairs landing microseconds apart,
        // most visibly the trailing space. Well below human-noticeable
        // for a 5-10 keystroke replay.
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
            // The last key is the boundary the user typed, and we react
            // to its *press* within ~10 ms — so it is still physically
            // held down here, and injecting a press for an already-down
            // key is a no-op at the compositor: the "space gets cut"
            // report. Emitting a release first (harmless if they already
            // let go) makes the following press a real down edge; the
            // user's own release then lands on an already-up key.
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

    /// Modifiers down, key tapped, modifiers up in reverse — the order
    /// a person's hand produces, and the order a compositor's shortcut
    /// matcher expects.
    ///
    /// The releases are not optional bookkeeping: a modifier left down
    /// turns every following keystroke of the replay into a shortcut,
    /// and the correction then looks like it never happened. Measured
    /// clean on GNOME 49 and MATE, 2026-08-24 — the chord switched and
    /// the next characters came out as characters.
    fn send_chord(&self, chord: SwitchChord) -> Result<(), InputError> {
        self.ensure_device()?;
        let mut g = self.device.lock();
        let dev = g
            .as_mut()
            .ok_or_else(|| InputError::Os("uinput device not initialised".into()))?;
        let step = Duration::from_millis(8);
        let mut mods: Vec<KeyCode> = Vec::new();
        if chord.ctrl {
            mods.push(KeyCode::KEY_LEFTCTRL);
        }
        if chord.shift {
            mods.push(KeyCode::KEY_LEFTSHIFT);
        }
        if chord.alt {
            mods.push(KeyCode::KEY_LEFTALT);
        }
        if chord.meta {
            mods.push(KeyCode::KEY_LEFTMETA);
        }
        for kc in &mods {
            press(dev, &self.emitted, *kc)?;
            thread::sleep(step);
        }
        // A bare-modifier chord (`Alt+Shift`, `Shift+Shift`) carries no
        // key of its own: the second modifier *is* the key.
        if chord.scancode != 0 {
            tap(dev, &self.emitted, KeyCode::new(chord.scancode as u16))?;
            thread::sleep(step);
        }
        for kc in mods.iter().rev() {
            release(dev, &self.emitted, *kc)?;
            thread::sleep(step);
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

        // The GTK/Qt unicode-input combo, Ctrl+Shift+U <hex> Space: the
        // standard Linux-wide "type a Unicode codepoint" sequence, good
        // in Firefox, Chromium, GTK and Qt. Terminal emulators that
        // disable it are deliberately not covered.
        for c in text.chars() {
            let cp = c as u32;
            let hex = format!("{cp:x}");
            press(dev, &self.emitted, KeyCode::KEY_LEFTCTRL)?;
            press(dev, &self.emitted, KeyCode::KEY_LEFTSHIFT)?;
            tap(dev, &self.emitted, KeyCode::KEY_U)?;
            release(dev, &self.emitted, KeyCode::KEY_LEFTSHIFT)?;
            release(dev, &self.emitted, KeyCode::KEY_LEFTCTRL)?;
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
