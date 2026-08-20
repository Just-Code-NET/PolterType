//! Device discovery and the blocking drain loop.

use super::*;
use crate::linux::access::{EVENT_DEVICE_DIR, NodeFacts, ScanFacts};
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

/// Every `/dev/input/event*` node, sorted so logs and rescans are
/// reproducible.
pub(crate) fn event_nodes() -> std::io::Result<Vec<PathBuf>> {
    let mut nodes: Vec<PathBuf> = std::fs::read_dir(EVENT_DEVICE_DIR)?
        .flatten()
        .map(|e| e.path())
        .filter(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("event"))
        })
        .collect();
    nodes.sort();
    Ok(nodes)
}

/// The initial scan, which also has to answer *why* when it finds
/// nothing.
///
/// Deliberately not `evdev::enumerate()`: that swallows every open
/// error, so a total permission failure and a machine with no keyboard
/// are indistinguishable — and telling those apart is the difference
/// between "log out and back in" and "run the setup script" (#31).
pub(crate) fn open_keyboard_devices() -> (Vec<OpenDevice>, ScanFacts) {
    let mut facts = ScanFacts::default();
    let nodes = match event_nodes() {
        Ok(nodes) => nodes,
        Err(e) => {
            facts.first_error = Some(e.to_string());
            return (Vec::new(), facts);
        }
    };
    facts.nodes = Some(nodes.len());
    let mut open = Vec::new();
    for path in nodes {
        match Device::open(&path) {
            Ok(dev) => {
                facts.opened += 1;
                if let Some(od) = accept_device(path, dev, true) {
                    facts.keyboards += usize::from(od.gate.is_keyboard);
                    open.push(od);
                }
            }
            Err(e) if facts.first_error.is_none() => {
                facts.first_error = Some(e.to_string());
                facts.sample = NodeFacts::of(&path);
            }
            Err(_) => {}
        }
    }
    (open, facts)
}

/// Open keyboards that appeared since the last scan — a hot-plugged
/// keyboard, a Bluetooth one powered back on, or our own emitter.
///
/// Deliberately not `evdev::enumerate()`: opening every node under
/// `/dev/input` and reading its capabilities costs 70–140 ms on the
/// thread that reads key events. Paying it every 2 s left the engine
/// blind ~5 % of the time, events arriving late in a burst exactly
/// where the correction logic is timing-sensitive.
///
/// Both halves matter: forgetting nothing makes a replugged keyboard
/// invisible (its node number is reused), forgetting everything
/// re-opens a dozen sound cards every 2 s.
pub(crate) fn plan_rescan(
    present: &HashSet<PathBuf>,
    known: &HashSet<PathBuf>,
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut fresh: Vec<PathBuf> = present.difference(known).cloned().collect();
    let mut forgotten: Vec<PathBuf> = known.difference(present).cloned().collect();
    // Deterministic order keeps logs and tests readable.
    fresh.sort();
    forgotten.sort();
    (fresh, forgotten)
}

/// `known` is every path already judged — opened or rejected — updated
/// in place. Judging costs an open plus a capability read, and most
/// nodes are sound cards and power buttons, so each is judged once.
/// Paths that disappear are forgotten, which is what makes a device
/// reappearing at the same node get looked at again.
pub(crate) fn open_new_keyboard_devices(known: &mut HashSet<PathBuf>) -> Vec<OpenDevice> {
    let Ok(nodes) = event_nodes() else {
        return Vec::new();
    };
    let present: HashSet<PathBuf> = nodes.into_iter().collect();
    let (fresh, forgotten) = plan_rescan(&present, known);
    for p in forgotten {
        known.remove(&p);
    }
    fresh
        .into_iter()
        .filter_map(|path| {
            known.insert(path.clone());
            let dev = Device::open(&path).ok()?;
            accept_device(path, dev, false)
        })
        .collect()
}

/// Shared gate for both scans: keep the device only if it is something
/// the engine wants to read, and make it safe to poll. `log_skips` is
/// on for the initial scan only — the rescan would otherwise re-log
/// every sound card / power button forever.
fn accept_device(path: PathBuf, dev: Device, log_skips: bool) -> Option<OpenDevice> {
    {
        {
            // A device advertising KEY_A is a keyboard. Devices with
            // BTN_LEFT are opened too: a click usually moves the caret,
            // which the engine must know about or its buffer silently
            // diverges from the screen. Only button presses are read.
            let types = dev.supported_keys();
            let is_keyboard = types.is_some_and(|k| k.contains(KeyCode::KEY_A));
            let wanted = is_keyboard || types.is_some_and(|k| k.contains(KeyCode::BTN_LEFT));
            let name = dev.name().unwrap_or("?").to_owned();
            if !wanted {
                if log_skips {
                    debug!(?path, name = %name, "evdev: skipped (no KEY_A / BTN_LEFT)");
                }
                return None;
            }
            // `fetch_events` blocks by default, and `drain_devices`
            // walks every device on one thread — so the first quiet
            // device would deadlock the loop for ever. Non-blocking
            // makes it return `WouldBlock`, which the loop handles.
            if let Err(e) = set_nonblocking(&dev) {
                warn!(?path, name = %name, ?e, "evdev: failed to set O_NONBLOCK — dropping");
                return None;
            }
            debug!(?path, name = %name, "evdev: opened keyboard");
            // Both signals: the name (also covers another instance's
            // emitter) and the node identity recorded at creation
            // (immune to name drift). Grabbing our own device funnels
            // the whole session's input into this process.
            let is_ours = name == EMITTER_DEVICE_NAME || own_nodes::is_own(&path);
            Some(OpenDevice {
                path,
                dev,
                gate: GateState {
                    is_ours,
                    is_keyboard,
                    ..GateState::default()
                },
            })
        }
    }
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
    gate: Arc<EvdevGate>,
) {
    // Naive multi-device polling; epoll-based fan-in is a later
    // optimisation.
    //
    // Modifier state is tracked in-loop because evdev gives raw
    // press/release pairs and the engine needs to know whether `K` was
    // typed shifted (`Lfdfq` → `Давай`, not `давай`). Caps Lock inverts
    // the same flag.
    let mut shift_down = false;
    let mut ctrl_down = false;
    let mut alt_down = false;
    let mut super_down = false;
    let mut caps_on = false;
    // Cheap enough at 2 s, and well below the time a human takes to
    // reconnect a device and start typing.
    let rescan_every = Duration::from_secs(2);
    let mut last_rescan = Instant::now();
    // Every `/dev/input/event*` path already judged, so the rescan only
    // ever opens something genuinely new.
    let mut known_paths: HashSet<PathBuf> = devices.iter().map(|od| od.path.clone()).collect();
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
                        if let Some(out_ev) = translate(&ev, modifiers, od.gate.is_ours) {
                            got_any = true;
                            od.gate.last_event = Some(Instant::now());
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
        // Remove dead devices high-index-first so earlier indices stay
        // valid. Forget their paths as well: `/dev/input` reuses event
        // nodes, so the next device to appear at that number is a
        // different one and has to be judged afresh — otherwise
        // unplugging and replugging a keyboard silently loses it.
        for idx in dead.into_iter().rev() {
            let gone = devices.swap_remove(idx);
            known_paths.remove(&gone.path);
        }
        // The thread stays alive even when `devices` is empty (every
        // keyboard unplugged) so this rescan can revive it.
        if last_rescan.elapsed() >= rescan_every {
            last_rescan = Instant::now();
            let fresh = open_new_keyboard_devices(&mut known_paths);
            if !fresh.is_empty() {
                info!(
                    count = fresh.len(),
                    "evdev: picked up new keyboard device(s)"
                );
                devices.extend(fresh);
            }
        }
        // Take / drop the correction-time grabs *after* reading, never
        // before: dropping one while events the grab captured are
        // still in a device buffer would strand them — read by nobody,
        // typed out by nobody, gone from the user's text.
        gate.service(&mut devices);
        if !got_any {
            thread::sleep(Duration::from_millis(2));
        }
    }
    // Never hand the devices back to the kernel still grabbed.
    gate.release_all(&mut devices);
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

/// `from_us` marks events read back off our own uinput device. Behind a
/// remapper our events return through *its* virtual keyboard untagged —
/// that is what the engine's echo queue is for — but on a direct stack
/// this flag identifies them exactly, which is the difference between
/// swallowing our own replay and swallowing a user keystroke that
/// happens to share a scancode.
pub(crate) fn translate(ev: &InputEvent, modifiers: Modifiers, from_us: bool) -> Option<KeyEvent> {
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
            injected: from_us,
            timestamp_ms: 0,
        });
    }
    let scancode = evdev_to_sc1(evdev_code);
    Some(KeyEvent {
        vk: evdev_code,
        scancode,
        direction,
        modifiers,
        injected: from_us,
        timestamp_ms: 0,
    })
}
