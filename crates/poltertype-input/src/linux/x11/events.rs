//! Connecting, selecting XInput2 raw events, and draining them.

use super::codes::*;
use super::consts::*;
use super::types::*;
use crate::{InputError, KeyDirection, KeyEvent};
use crossbeam_channel::Sender;
use poltertype_types::SC_POINTER_BUTTON;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use tracing::{debug, info, trace, warn};
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xinput::{self, ConnectionExt as _};
use x11rb::rust_connection::RustConnection;

/// XInput2's "all master devices" pseudo-device. Selecting on masters
/// (rather than `Device::ALL`) means a keystroke is reported once; on
/// `ALL` the same press arrives twice — once for the physical slave
/// device and once for the master it is attached to.
fn all_master_devices() -> xinput::DeviceId {
    xinput::Device::ALL_MASTER.into()
}

/// Open the display and select raw key/button events on the root window.
///
/// Raw events are the only X11 mechanism that sees keystrokes destined
/// for *other* clients without grabbing the keyboard away from them —
/// a grab would make us the only recipient and break typing entirely.
pub(crate) fn connect_and_select() -> Result<RustConnection, InputError> {
    let (conn, screen_num) = x11rb::connect(None)
        .map_err(|e| InputError::Os(format!("x11 connect (is DISPLAY set?): {e}")))?;

    let ver = conn
        .xinput_xi_query_version(2, 0)
        .map_err(|e| InputError::Os(format!("XInput2 query version: {e}")))?
        .reply()
        .map_err(|e| InputError::Unsupported(format!("XInput2 not available: {e}")))?;
    if ver.major_version < 2 {
        return Err(InputError::Unsupported(format!(
            "XInput2 required, server offers {}.{}",
            ver.major_version, ver.minor_version
        )));
    }

    let root = conn
        .setup()
        .roots
        .get(screen_num)
        .ok_or_else(|| InputError::Os(format!("no screen {screen_num}")))?
        .root;

    let mask = xinput::EventMask {
        deviceid: all_master_devices(),
        mask: vec![
            xinput::XIEventMask::RAW_KEY_PRESS
                | xinput::XIEventMask::RAW_KEY_RELEASE
                | xinput::XIEventMask::RAW_BUTTON_PRESS,
        ],
    };
    conn.xinput_xi_select_events(root, &[mask])
        .map_err(|e| InputError::Os(format!("XISelectEvents: {e}")))?;
    conn.flush()
        .map_err(|e| InputError::Os(format!("x11 flush: {e}")))?;

    info!(
        version = format!("{}.{}", ver.major_version, ver.minor_version),
        "XInput2 raw events selected on root window"
    );
    Ok(conn)
}

pub(crate) fn drain_events(conn: RustConnection, sink: Sender<KeyEvent>, stop: Arc<AtomicBool>) {
    let mut mods = ModState::default();
    while !stop.load(Ordering::SeqCst) {
        match conn.poll_for_event() {
            Ok(Some(ev)) => {
                if let Some(out) = translate(&ev, &mut mods) {
                    trace!(vk = out.vk, dir = ?out.direction, "x11 raw event");
                    if sink.try_send(out).is_err() {
                        debug!("x11 sink full — dropping event");
                    }
                }
            }
            Ok(None) => thread::sleep(POLL_IDLE),
            // The connection is gone (X server shut down, session
            // ended). There is nothing to recover to — exit the thread
            // rather than spin on a dead socket forever.
            Err(e) => {
                warn!(?e, "x11 connection lost — listener thread exiting");
                return;
            }
        }
    }
    info!("x11 listener thread exiting");
}

pub(crate) fn translate(ev: &Event, mods: &mut ModState) -> Option<KeyEvent> {
    match ev {
        Event::XinputRawKeyPress(e) => {
            let evdev = x11_to_evdev(e.detail)?;
            // Auto-repeat is a real press as far as the engine is
            // concerned, but it must not re-toggle Caps Lock — holding
            // Caps down would otherwise flap the flag many times a
            // second.
            if !e.flags.contains(xinput::KeyEventFlags::KEY_REPEAT) {
                mods.press(evdev);
            }
            Some(key_event(evdev, KeyDirection::Press, mods.snapshot()))
        }
        Event::XinputRawKeyRelease(e) => {
            let evdev = x11_to_evdev(e.detail)?;
            mods.release(evdev);
            Some(key_event(evdev, KeyDirection::Release, mods.snapshot()))
        }
        // A click usually moves the caret. Report the press as a
        // caret-jump marker so the engine abandons its word buffer
        // instead of correcting a word the user has since clicked away
        // from. Releases are noise; we never select them.
        Event::XinputRawButtonPress(e) => {
            if !is_caret_jump_button(e.detail) {
                return None;
            }
            Some(KeyEvent {
                vk: e.detail,
                scancode: SC_POINTER_BUTTON,
                direction: KeyDirection::Press,
                modifiers: mods.snapshot(),
                injected: false,
                timestamp_ms: 0,
            })
        }
        _ => None,
    }
}

fn key_event(evdev: u32, direction: KeyDirection, modifiers: crate::Modifiers) -> KeyEvent {
    KeyEvent {
        vk: evdev,
        // evdev codes and Win SC Set-1 scancodes coincide across every
        // row the word buffer keeps — the same identity the evdev
        // backend relies on (`wayland::evdev_to_sc1`).
        scancode: evdev,
        direction,
        modifiers,
        // XTest echoes arrive here indistinguishable from real typing;
        // `injected` would be a lie. The engine filters our own events
        // via the emitter's echo log instead (see `KeyEmitter::take_emitted`).
        injected: false,
        timestamp_ms: 0,
    }
}
