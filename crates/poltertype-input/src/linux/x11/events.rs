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
use std::time::Instant;
use tracing::{debug, info, trace, warn};
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xinput::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{self, ConnectionExt as _};
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
    let mut last_resync = Instant::now();
    let mut last_caps_resync = Instant::now();
    // Seed the latch: the session may well have started with Caps Lock
    // already on, and nothing else would ever tell us.
    resync_caps(&conn, &mut mods, &mut last_caps_resync, true);
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
            Ok(None) => {
                resync_caps(&conn, &mut mods, &mut last_caps_resync, false);
                resync_modifiers(&conn, &mut mods, &mut last_resync);
                thread::sleep(POLL_IDLE);
            }
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

/// Ask the server which modifier keys are *actually* down, and correct
/// our latched view if it disagrees.
///
/// Edges go missing: any client holding an active keyboard grab stops
/// XInput2 raw events reaching us while it holds one — measured on
/// X.org, three key taps produced nine raw events without a grab and
/// **zero** during one, with no error. A modifier left latched makes
/// `Modifiers::is_command()` read every later keystroke as a shortcut,
/// and the app goes quiet until restarted. Reproduced with a desktop
/// keybinding bound to a bare modifier — Cinnamon's per-layout switch
/// shortcuts, which PolterType makes fire often by changing the layout
/// under the user
/// ([#26](https://github.com/Just-Code-NET/PolterType/issues/26)).
///
/// `XQueryKeymap` answers from the server's own device state and keeps
/// working through a foreign grab. One round-trip per
/// [`MOD_RESYNC_INTERVAL`], and only while we believe a modifier is
/// held.
fn resync_modifiers(conn: &RustConnection, mods: &mut ModState, last: &mut Instant) {
    if !mods.any_held() || last.elapsed() < MOD_RESYNC_INTERVAL {
        return;
    }
    *last = Instant::now();
    // A failed query is not worth a log line on every idle round: the
    // connection erroring for real is caught by the caller's `Err`
    // arm, which does report it.
    let Ok(Ok(reply)) = conn.query_keymap().map(|cookie| cookie.reply()) else {
        return;
    };
    if mods.resync(&reply.keys) {
        debug!(
            mods = ?mods.snapshot(),
            "modifier state corrected from XQueryKeymap — an edge was missed, \
             most likely to a keyboard grab by another client"
        );
    }
}

/// Re-read the Caps Lock latch from the server after a Caps Lock edge
/// (or once at startup, with `force`).
///
/// `QueryPointer` answers with the effective modifier mask, `Lock`
/// included — the state xkb will apply to our replayed keystrokes.
/// Asking is the only way to know: the key is often bound to Escape,
/// Ctrl or the layout switch, where pressing it latches nothing.
fn resync_caps(conn: &RustConnection, mods: &mut ModState, last: &mut Instant, force: bool) {
    // The edge is not the only way the latch moves — a compositor-level
    // remapper or `xdotool key Caps_Lock` change it with no key event
    // reaching us at all — so the interval is what keeps a wrong latch
    // from outliving the session. Idle rounds only, so an idle keyboard
    // still never asks.
    if !mods.take_caps_stale() && !force && last.elapsed() < MOD_RESYNC_INTERVAL {
        return;
    }
    *last = Instant::now();
    let Some(root) = conn.setup().roots.first().map(|s| s.root) else {
        return;
    };
    let Ok(Ok(reply)) = conn.query_pointer(root).map(|cookie| cookie.reply()) else {
        return;
    };
    let caps = reply.mask.contains(xproto::KeyButMask::LOCK);
    mods.set_caps(caps);
    debug!(caps, "Caps Lock latch read from the server");
}

pub(crate) fn translate(ev: &Event, mods: &mut ModState) -> Option<KeyEvent> {
    match ev {
        Event::XinputRawKeyPress(e) => {
            let evdev = x11_to_evdev(e.detail)?;
            // Auto-repeat is a real press as far as the engine is
            // concerned, but holding Caps Lock down must not ask the
            // server for the latch hundreds of times a second.
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
