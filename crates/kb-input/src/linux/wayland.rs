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
use tracing::{debug, info, trace, warn};

use crate::{InputError, InputListener, KeyDirection, KeyEmitter, KeyEvent, Modifiers, ReplayKey};

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
        .filter_map(|(path, dev)| {
            // Heuristic: a device that advertises KEY_A is a keyboard.
            let is_keyboard = dev
                .supported_keys()
                .is_some_and(|k| k.contains(KeyCode::KEY_A));
            let name = dev.name().unwrap_or("?").to_owned();
            if !is_keyboard {
                debug!(?path, name = %name, "evdev: skipped (no KEY_A)");
                return None;
            }
            // `evdev::Device::fetch_events` is a blocking read by
            // default. Our `drain_devices` loop walks every device
            // in turn on a single thread, so the first quiet device
            // (a HID keyboard on a mouse that nobody is typing on,
            // a sleep button, …) would deadlock the loop forever.
            // Flip the FD to non-blocking so `fetch_events` returns
            // `WouldBlock` instead of waiting — the loop already
            // handles that branch.
            if let Err(e) = set_nonblocking(&dev) {
                warn!(?path, name = %name, ?e, "evdev: failed to set O_NONBLOCK — dropping");
                return None;
            }
            debug!(?path, name = %name, "evdev: opened keyboard");
            Some(dev)
        })
        .collect()
}

fn set_nonblocking(dev: &Device) -> std::io::Result<()> {
    let fd = dev.as_raw_fd();
    // SAFETY: we hold a borrow of `dev`, so the FD is valid for the
    // duration of these two syscalls.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags < 0 {
            return Err(std::io::Error::last_os_error());
        }
        if libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

fn drain_devices(mut devices: Vec<Device>, sink: Sender<KeyEvent>, stop: Arc<AtomicBool>) {
    // Naive multi-device polling — for v0.1 we just spin a small
    // loop that asks each device for events. epoll-based fan-in is a
    // v0.1.x optimisation.
    //
    // We track modifier state in-loop because evdev gives us raw
    // press/release pairs and the engine downstream needs to know
    // whether `K` was typed shifted (`Lfdfq` -> `Давай`, not `давай`).
    // Caps Lock is handled the same way — it inverts the shift flag
    // for the produced character.
    let mut shift_down = false;
    let mut ctrl_down = false;
    let mut alt_down = false;
    let mut super_down = false;
    let mut caps_on = false;
    while !stop.load(Ordering::SeqCst) {
        let mut got_any = false;
        for dev in devices.iter_mut() {
            match dev.fetch_events() {
                Ok(events) => {
                    for ev in events {
                        update_modifiers(
                            &ev,
                            &mut shift_down,
                            &mut ctrl_down,
                            &mut alt_down,
                            &mut super_down,
                            &mut caps_on,
                        );
                        let modifiers = Modifiers {
                            shift: shift_down ^ caps_on,
                            control: ctrl_down,
                            alt: alt_down,
                            meta: super_down,
                        };
                        if let Some(out_ev) = translate(&ev, modifiers) {
                            got_any = true;
                            trace!(vk = out_ev.vk, dir = ?out_ev.direction, "evdev event");
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

fn update_modifiers(
    ev: &InputEvent,
    shift: &mut bool,
    ctrl: &mut bool,
    alt: &mut bool,
    super_: &mut bool,
    caps: &mut bool,
) {
    if ev.event_type() != EventType::KEY {
        return;
    }
    let pressed = ev.value() == 1; // ignore autorepeat (2) for modifier transitions
    let released = ev.value() == 0;
    match KeyCode::new(ev.code()) {
        KeyCode::KEY_LEFTSHIFT | KeyCode::KEY_RIGHTSHIFT => {
            if pressed {
                *shift = true;
            } else if released {
                *shift = false;
            }
        }
        KeyCode::KEY_LEFTCTRL | KeyCode::KEY_RIGHTCTRL => {
            if pressed {
                *ctrl = true;
            } else if released {
                *ctrl = false;
            }
        }
        KeyCode::KEY_LEFTALT | KeyCode::KEY_RIGHTALT => {
            if pressed {
                *alt = true;
            } else if released {
                *alt = false;
            }
        }
        KeyCode::KEY_LEFTMETA | KeyCode::KEY_RIGHTMETA => {
            if pressed {
                *super_ = true;
            } else if released {
                *super_ = false;
            }
        }
        // Caps Lock toggles on press, not on release.
        KeyCode::KEY_CAPSLOCK if pressed => *caps = !*caps,
        _ => {}
    }
}

fn translate(ev: &InputEvent, modifiers: Modifiers) -> Option<KeyEvent> {
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
        modifiers,
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
                InputEvent::new(EventType::KEY.0, KeyCode::KEY_BACKSPACE.0, 1),
            )?;
            thread::sleep(step);
            emit_one(
                dev,
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
        // `hyprctl switchxkblayout` returns instantly but the
        // compositor still needs a moment to propagate the new xkb
        // state to the focused client; firing scancodes too quickly
        // makes them land under the old layout (and you see the
        // original `lfdfq` rather than `давай`). 30 ms is far below
        // human-noticeable but enough on this stack.
        thread::sleep(Duration::from_millis(30));
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
        // The very last key in the replay is the boundary the user
        // typed (almost always Space). libinput / keyd appear to
        // sometimes coalesce a short press-then-release at the tail
        // of an event burst into nothing, which is the symptom of
        // the "missing space between two corrected words" report.
        // Holding the boundary key down 20 ms before release is far
        // below human-noticeable but enough that no downstream
        // filter treats it as a debouncer event.
        let last_hold = Duration::from_millis(20);
        let last_idx = keys.len() - 1;
        for (i, rk) in keys.iter().enumerate() {
            let kc = rk.scancode as u16;
            debug!(scancode = rk.scancode, shift = rk.shift, "uinput key");
            if rk.shift {
                emit_one(
                    dev,
                    InputEvent::new(EventType::KEY.0, KeyCode::KEY_LEFTSHIFT.0, 1),
                )?;
                thread::sleep(step);
            }
            emit_one(dev, InputEvent::new(EventType::KEY.0, kc, 1))?;
            thread::sleep(if i == last_idx { last_hold } else { step });
            emit_one(dev, InputEvent::new(EventType::KEY.0, kc, 0))?;
            thread::sleep(step);
            if rk.shift {
                emit_one(
                    dev,
                    InputEvent::new(EventType::KEY.0, KeyCode::KEY_LEFTSHIFT.0, 0),
                )?;
                thread::sleep(step);
            }
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

fn emit_one(dev: &mut VirtualDevice, ev: InputEvent) -> Result<(), InputError> {
    dev.emit(&[ev])
        .map_err(|e| InputError::Os(format!("uinput emit: {e}")))
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
/// For the alphanumeric / number / boundary rows we care about,
/// Linux's evdev `KEY_*` codes coincide with SC Set-1 (e.g. evdev
/// 1 = Esc = SC1 0x01, evdev 30 = A = SC1 0x1E). Extended keys
/// (arrows, NumLock area) diverge — they're discarded by the
/// engine's word-buffer classifier so we don't bother re-mapping
/// them yet. Phase 6.x can add the table when X11 fallback lands.
///
/// Reference: https://www.kernel.org/doc/Documentation/input/event-codes.txt
fn evdev_to_sc1(evdev: u32) -> u32 {
    evdev
}
