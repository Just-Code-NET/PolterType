//! Low-level XTest emission helpers.

use super::codes::*;
use super::consts::*;
use super::types::*;
use crate::{EmittedKey, InputError, KeyDirection};
use std::thread;
use x11rb::connection::{Connection, RequestConnection as _};
use x11rb::protocol::xproto::{ConnectionExt as _, KEY_PRESS_EVENT, KEY_RELEASE_EVENT, Keysym};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

/// `NoSymbol` — the keysym that means "this slot is unbound".
const NO_SYMBOL: Keysym = 0;

pub(crate) fn connect_xtest() -> Result<X11Conn, InputError> {
    let (conn, screen_num) = x11rb::connect(None)
        .map_err(|e| InputError::Os(format!("x11 connect (is DISPLAY set?): {e}")))?;
    // XTest is near-universal but can be compiled out of a hardened
    // server, and a clear error here beats every emit failing later.
    conn.extension_information(x11rb::protocol::xtest::X11_EXTENSION_NAME)
        .map_err(|e| InputError::Os(format!("x11 extension query: {e}")))?
        .ok_or_else(|| InputError::Unsupported("X server has no XTEST extension".into()))?;
    let root = conn
        .setup()
        .roots
        .get(screen_num)
        .ok_or_else(|| InputError::Os(format!("no screen {screen_num}")))?
        .root;
    Ok(X11Conn { conn, root })
}

/// Inject one key edge and, on success, record it in the echo log.
///
/// Recording only after the request went out means a failed emit never
/// leaves a phantom entry that would later eat a real user keystroke.
pub(crate) fn emit_one(
    c: &X11Conn,
    log: &parking_lot::Mutex<Vec<EmittedKey>>,
    evdev: u32,
    press: bool,
) -> Result<(), InputError> {
    let keycode = evdev_to_x11(evdev)
        .ok_or_else(|| InputError::Os(format!("keycode {evdev} out of range")))?;
    let type_ = if press {
        KEY_PRESS_EVENT
    } else {
        KEY_RELEASE_EVENT
    };
    c.conn
        .xtest_fake_input(type_, keycode, 0, c.root, 0, 0, 0)
        .map_err(|e| InputError::Os(format!("XTestFakeInput: {e}")))?;
    c.conn
        .flush()
        .map_err(|e| InputError::Os(format!("x11 flush: {e}")))?;
    log.lock().push(EmittedKey {
        // The echo log is matched against events coming back from the
        // listener, which reports evdev codes — so log evdev, not the
        // X11 keycode we just put on the wire.
        scancode: evdev,
        direction: if press {
            KeyDirection::Press
        } else {
            KeyDirection::Release
        },
    });
    Ok(())
}

pub(crate) fn press(
    c: &X11Conn,
    log: &parking_lot::Mutex<Vec<EmittedKey>>,
    evdev: u32,
) -> Result<(), InputError> {
    emit_one(c, log, evdev, true)
}

pub(crate) fn release(
    c: &X11Conn,
    log: &parking_lot::Mutex<Vec<EmittedKey>>,
    evdev: u32,
) -> Result<(), InputError> {
    emit_one(c, log, evdev, false)
}

pub(crate) fn tap(
    c: &X11Conn,
    log: &parking_lot::Mutex<Vec<EmittedKey>>,
    evdev: u32,
) -> Result<(), InputError> {
    press(c, log, evdev)?;
    thread::sleep(KEY_STEP);
    release(c, log, evdev)?;
    thread::sleep(KEY_STEP);
    Ok(())
}

/// Block until the server has processed everything we've sent.
///
/// `GetInputFocus` is the traditional no-op round-trip: we don't want
/// the focus, we want the reply, because waiting for it proves the
/// preceding requests (a keymap change, say) have been applied.
pub(crate) fn sync(c: &X11Conn) -> Result<(), InputError> {
    c.conn
        .get_input_focus()
        .map_err(|e| InputError::Os(format!("x11 sync: {e}")))?
        .reply()
        .map_err(|e| InputError::Os(format!("x11 sync reply: {e}")))?;
    Ok(())
}

/// Find a keycode that the current keymap leaves completely unbound.
///
/// To type a character no physical key produces, we borrow such a
/// keycode, point it at the keysym we want, tap it, and put it back.
/// Searching from the top of the range first is deliberate: the high
/// keycodes are where `xkeyboard-config` leaves gaps, so we disturb the
/// least-used corner of the keymap.
pub(crate) fn find_spare_keycode(conn: &RustConnection) -> Result<(u8, u8), InputError> {
    let setup = conn.setup();
    let min = setup.min_keycode;
    let max = setup.max_keycode;
    let count = max - min + 1;

    let mapping = conn
        .get_keyboard_mapping(min, count)
        .map_err(|e| InputError::Os(format!("GetKeyboardMapping: {e}")))?
        .reply()
        .map_err(|e| InputError::Os(format!("GetKeyboardMapping reply: {e}")))?;

    let per = mapping.keysyms_per_keycode as usize;
    if per == 0 {
        return Err(InputError::Os(
            "keymap reports 0 keysyms per keycode".into(),
        ));
    }

    for (i, syms) in mapping.keysyms.chunks(per).enumerate().rev() {
        if syms.iter().all(|&s| s == NO_SYMBOL) {
            let keycode = min
                + u8::try_from(i).map_err(|_| {
                    InputError::Os("keymap larger than the X11 keycode range".into())
                })?;
            return Ok((keycode, mapping.keysyms_per_keycode));
        }
    }
    Err(InputError::Os(
        "no unbound keycode to borrow for Unicode input — keymap is full".into(),
    ))
}

/// Point `keycode` at `keysym` in every shift level.
///
/// Binding all levels to the same symbol means the character comes out
/// the same whether or not the user happens to be holding Shift while
/// the correction fires.
pub(crate) fn bind_keysym(
    c: &X11Conn,
    keycode: u8,
    per: u8,
    keysym: Keysym,
) -> Result<(), InputError> {
    let syms = vec![keysym; per as usize];
    c.conn
        .change_keyboard_mapping(1, keycode, per, &syms)
        .map_err(|e| InputError::Os(format!("ChangeKeyboardMapping: {e}")))?;
    sync(c)?;
    thread::sleep(REMAP_SETTLE);
    Ok(())
}

/// Hand the borrowed keycode back, unbound as we found it.
pub(crate) fn unbind_keysym(c: &X11Conn, keycode: u8, per: u8) -> Result<(), InputError> {
    bind_keysym(c, keycode, per, NO_SYMBOL)
}
