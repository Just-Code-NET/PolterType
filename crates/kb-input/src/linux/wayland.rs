//! Wayland-friendly keyboard listener via `evdev`, plus `uinput`
//! emitter for replays.
//!
//! ## Listener
//!
//! Open every `/dev/input/event*` device that advertises keyboard
//! capability and read events from all of them on a worker thread.
//!
//! ## Emitter
//!
//! Create a single `uinput` virtual keyboard at start, post Backspace
//! and arbitrary Unicode codepoints to it. Unicode entry on
//! plain-evdev is best-effort: most real GUI apps respect the
//! compose-XKB unicode-input combo (`Ctrl+Shift+U <hex> Enter`),
//! which we drive synthetically.

#![allow(unused_imports, dead_code)] // Linux-only; gated by cfg in lib.rs.

use std::os::fd::AsRawFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use crossbeam_channel::Sender;
use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, Device, EventType, InputEvent, KeyCode};
use tracing::{debug, info, warn};

use crate::{InputError, InputListener, KeyDirection, KeyEmitter, KeyEvent, Modifiers};

// ─── Listener ────────────────────────────────────────────────────────

pub struct EvdevListener {
    stop: Arc<AtomicBool>,
}

impl EvdevListener {
    pub fn new() -> Self {
        Self {
            stop: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl InputListener for EvdevListener {
    fn start(&mut self, sink: Sender<KeyEvent>) -> Result<(), InputError> {
        let devices = open_keyboard_devices();
        if devices.is_empty() {
            return Err(InputError::Os(
                "no readable keyboard devices in /dev/input/* — \
                 run scripts/setup-linux.sh to grant access"
                    .into(),
            ));
        }
        info!(count = devices.len(), "opened evdev keyboard devices");

        let stop = Arc::clone(&self.stop);
        thread::Builder::new()
            .name("kb-input-evdev".into())
            .spawn(move || drain_devices(devices, sink, stop))
            .map_err(|e| InputError::Os(format!("spawn evdev thread: {e}")))?;
        Ok(())
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    fn backend_name(&self) -> &'static str {
        "linux-wayland-evdev"
    }
}

fn open_keyboard_devices() -> Vec<Device> {
    // evdev 0.13's `enumerate()` is infallible — it yields whatever
    // is openable and silently skips the rest. Permission errors on
    // individual devices fall through, which is exactly what we want.
    evdev::enumerate()
        .filter_map(|(_path, dev)| {
            // Heuristic: a device that advertises KEY_A is a keyboard.
            let is_keyboard = dev
                .supported_keys()
                .is_some_and(|k| k.contains(KeyCode::KEY_A));
            if is_keyboard { Some(dev) } else { None }
        })
        .collect()
}

fn drain_devices(mut devices: Vec<Device>, sink: Sender<KeyEvent>, stop: Arc<AtomicBool>) {
    // Naive multi-device polling — for v0.1 we just spin a small
    // loop that asks each device for events. epoll-based fan-in is a
    // v0.1.x optimisation.
    while !stop.load(Ordering::SeqCst) {
        let mut got_any = false;
        for dev in devices.iter_mut() {
            match dev.fetch_events() {
                Ok(events) => {
                    for ev in events {
                        if let Some(out_ev) = translate(&ev) {
                            got_any = true;
                            if sink.try_send(out_ev).is_err() {
                                debug!("evdev sink full — dropping event");
                            }
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => warn!(?e, "evdev fetch_events"),
            }
        }
        if !got_any {
            thread::sleep(Duration::from_millis(2));
        }
    }
    info!("evdev listener thread exiting");
}

fn translate(ev: &InputEvent) -> Option<KeyEvent> {
    if ev.event_type() != EventType::KEY {
        return None;
    }
    let direction = match ev.value() {
        0 => KeyDirection::Release,
        1 | 2 => KeyDirection::Press, // 2 = autorepeat — treat as press.
        _ => return None,
    };
    let evdev_code = ev.code() as u32;
    let scancode = evdev_to_sc1(evdev_code);
    Some(KeyEvent {
        vk: evdev_code,
        scancode,
        direction,
        // evdev gives us the raw event — modifier state has to be
        // tracked from the event stream itself. v0.1 leaves the
        // modifiers empty and lets the engine treat command-shortcuts
        // as "any key". v0.1.x adds proper modifier tracking.
        modifiers: Modifiers::default(),
        injected: false,
        timestamp_ms: 0,
    })
}

// ─── Emitter ─────────────────────────────────────────────────────────

pub struct UinputEmitter {
    device: parking_lot::Mutex<Option<VirtualDevice>>,
}

impl UinputEmitter {
    pub fn new() -> Self {
        Self {
            device: parking_lot::Mutex::new(None),
        }
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
            .name("kb-switcher virtual keyboard")
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
        for _ in 0..n {
            let down = InputEvent::new(EventType::KEY.0, KeyCode::KEY_BACKSPACE.0, 1);
            let up = InputEvent::new(EventType::KEY.0, KeyCode::KEY_BACKSPACE.0, 0);
            dev.emit(&[down, up])
                .map_err(|e| InputError::Os(format!("uinput emit: {e}")))?;
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
            press(dev, KeyCode::KEY_LEFTCTRL)?;
            press(dev, KeyCode::KEY_LEFTSHIFT)?;
            tap(dev, KeyCode::KEY_U)?;
            release(dev, KeyCode::KEY_LEFTSHIFT)?;
            release(dev, KeyCode::KEY_LEFTCTRL)?;
            // Hex digits
            for ch in hex.chars() {
                if let Some(kc) = ascii_hex_to_keycode(ch) {
                    tap(dev, kc)?;
                }
            }
            tap(dev, KeyCode::KEY_SPACE)?;
        }
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "linux-uinput"
    }
}

fn press(dev: &mut VirtualDevice, k: KeyCode) -> Result<(), InputError> {
    dev.emit(&[InputEvent::new(EventType::KEY.0, k.0, 1)])
        .map_err(|e| InputError::Os(format!("uinput emit press: {e}")))
}
fn release(dev: &mut VirtualDevice, k: KeyCode) -> Result<(), InputError> {
    dev.emit(&[InputEvent::new(EventType::KEY.0, k.0, 0)])
        .map_err(|e| InputError::Os(format!("uinput emit release: {e}")))
}
fn tap(dev: &mut VirtualDevice, k: KeyCode) -> Result<(), InputError> {
    dev.emit(&[
        InputEvent::new(EventType::KEY.0, k.0, 1),
        InputEvent::new(EventType::KEY.0, k.0, 0),
    ])
    .map_err(|e| InputError::Os(format!("uinput emit tap: {e}")))
}

fn ascii_hex_to_keycode(c: char) -> Option<KeyCode> {
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
/// evdev's `KEY_*` constants are *almost* SC Set-1 + 8, so the easy
/// rule is `evdev - 8`. We special-case a few that don't follow it.
fn evdev_to_sc1(evdev: u32) -> u32 {
    if evdev >= 1 { evdev - 0 } else { evdev }
    // Note: evdev codes are documented at
    // https://www.kernel.org/doc/Documentation/input/event-codes.txt
    // The actual mapping evdev↔SC1 is:
    //   evdev 1   = SC1 0x01 (Esc)        ← identity
    //   evdev 30  = SC1 0x1E (A)          ← identity
    //   evdev 105 = SC1 0xE0 0x4B (LeftArrow extended)
    // For the rows we care about (alphanumeric main block) the codes
    // are equal up to a one-to-one correspondence and our function is
    // the identity. The XInput2 backend uses the same identity.
}
