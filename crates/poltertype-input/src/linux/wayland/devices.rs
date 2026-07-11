//! Device discovery and the blocking drain loop.

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

pub(crate) fn open_keyboard_devices() -> Vec<OpenDevice> {
    open_keyboard_devices_except(&HashSet::new(), true)
}

/// Open every keyboard `evdev` device whose path is not already in
/// `skip`. Used both for the initial scan (empty `skip`) and for the
/// periodic rescan that picks up hot-plugged / reconnected keyboards.
/// `log_skips` is on for the initial scan only — the 2 s rescan would
/// otherwise re-log every sound card / power button forever.
pub(crate) fn open_keyboard_devices_except(
    skip: &HashSet<PathBuf>,
    log_skips: bool,
) -> Vec<OpenDevice> {
    // evdev 0.13's `enumerate()` is infallible — it yields whatever
    // is openable and silently skips the rest. Permission errors on
    // individual devices fall through, which is exactly what we want.
    evdev::enumerate()
        .filter_map(|(path, dev)| {
            if skip.contains(&path) {
                return None;
            }
            // Heuristic: a device that advertises KEY_A is a keyboard.
            // Devices with BTN_LEFT (mice, touchpads) are opened too —
            // a click usually moves the caret, which the engine must
            // know about or its word buffer silently diverges from the
            // text on screen (the classic "half a word got corrected"
            // report). We only ever read button presses off them.
            let is_keyboard = dev
                .supported_keys()
                .is_some_and(|k| k.contains(KeyCode::KEY_A) || k.contains(KeyCode::BTN_LEFT));
            let name = dev.name().unwrap_or("?").to_owned();
            if !is_keyboard {
                if log_skips {
                    debug!(?path, name = %name, "evdev: skipped (no KEY_A / BTN_LEFT)");
                }
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
            Some(OpenDevice { path, dev })
        })
        .collect()
}

pub(crate) fn set_nonblocking(dev: &Device) -> std::io::Result<()> {
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

pub(crate) fn drain_devices(
    mut devices: Vec<OpenDevice>,
    sink: Sender<KeyEvent>,
    stop: Arc<AtomicBool>,
) {
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
    // Re-enumerate `/dev/input` on this cadence to pick up keyboards that
    // were plugged in (or a Bluetooth keyboard powered back on) after the
    // listener started. Cheap enough at 2 s; well below the time a human
    // takes to reconnect a device and start typing.
    let rescan_every = Duration::from_secs(2);
    let mut last_rescan = Instant::now();
    while !stop.load(Ordering::SeqCst) {
        let mut got_any = false;
        let mut dead = Vec::new();
        for (idx, od) in devices.iter_mut().enumerate() {
            match od.dev.fetch_events() {
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
                // A disconnected device (Bluetooth keyboard powered off,
                // USB unplugged) reports ENODEV on every poll. Left in
                // the list it would re-error a few hundred times a second
                // and flood the log forever, so drop it once and move on.
                Err(e) if e.raw_os_error() == Some(libc::ENODEV) => {
                    info!("evdev device disconnected — dropping it");
                    dead.push(idx);
                }
                Err(e) => warn!(?e, "evdev fetch_events"),
            }
        }
        // Remove dead devices high-index-first so earlier indices stay valid.
        for idx in dead.into_iter().rev() {
            devices.swap_remove(idx);
        }
        // Periodically re-enumerate so a reconnected keyboard is picked
        // back up. We keep the thread alive even when `devices` is empty
        // (every keyboard unplugged) so the rescan can revive it.
        if last_rescan.elapsed() >= rescan_every {
            last_rescan = Instant::now();
            let open: HashSet<PathBuf> = devices.iter().map(|od| od.path.clone()).collect();
            let fresh = open_keyboard_devices_except(&open, false);
            if !fresh.is_empty() {
                info!(
                    count = fresh.len(),
                    "evdev: picked up new keyboard device(s)"
                );
                devices.extend(fresh);
            }
        }
        if !got_any {
            thread::sleep(Duration::from_millis(2));
        }
    }
    info!("evdev listener thread exiting");
}

pub(crate) fn update_modifiers(
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

pub(crate) fn translate(ev: &InputEvent, modifiers: Modifiers) -> Option<KeyEvent> {
    if ev.event_type() != EventType::KEY {
        return None;
    }
    let direction = match ev.value() {
        0 => KeyDirection::Release,
        1 | 2 => KeyDirection::Press, // 2 = autorepeat — treat as press.
        _ => return None,
    };
    let evdev_code = ev.code() as u32;
    // Pointer buttons (BTN_LEFT..=BTN_TASK — mouse clicks, touchpad
    // taps): report the press as a caret-jump marker so the engine
    // abandons its word buffer. Releases are noise; drop them.
    // BTN_TOUCH (bare touchpad contact / finger motion) is deliberately
    // NOT reported — moving the pointer doesn't move the caret.
    if (272..=279).contains(&evdev_code) {
        if direction != KeyDirection::Press || ev.value() == 2 {
            return None;
        }
        return Some(KeyEvent {
            vk: evdev_code,
            scancode: SC_POINTER_BUTTON,
            direction: KeyDirection::Press,
            modifiers,
            injected: false,
            timestamp_ms: 0,
        });
    }
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
