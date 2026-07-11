//! `X11Emitter` — replays corrections via XTest.

use super::codes::*;
use super::consts::*;
use super::emit::*;
use super::types::*;
use crate::{EmittedKey, InputError, KeyEmitter, ReplayKey};
use std::thread;
use tracing::{debug, warn};

pub struct X11Emitter {
    conn: parking_lot::Mutex<Option<X11Conn>>,
    /// Log of every key edge actually injected since the last
    /// [`KeyEmitter::take_emitted`]. XTest events come back to us
    /// through XInput2 raw events looking exactly like real typing, so
    /// the engine match-and-consumes them off the key stream using this
    /// log — the same mechanism the uinput backend needs behind keyd.
    emitted: parking_lot::Mutex<Vec<EmittedKey>>,
}

impl X11Emitter {
    pub fn new() -> Self {
        let s = Self {
            conn: parking_lot::Mutex::new(None),
            emitted: parking_lot::Mutex::new(Vec::new()),
        };
        // Connect eagerly so a broken DISPLAY surfaces in the log at
        // startup rather than as a mystery failure on the user's first
        // correction. Failure is survivable — we retry on first use.
        if let Err(e) = s.ensure_conn() {
            warn!(?e, "x11 emitter connection deferred to first use");
        }
        s
    }

    fn ensure_conn(&self) -> Result<(), InputError> {
        let mut g = self.conn.lock();
        if g.is_none() {
            *g = Some(connect_xtest()?);
        }
        Ok(())
    }
}

impl Default for X11Emitter {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyEmitter for X11Emitter {
    fn send_backspaces(&self, n: usize) -> Result<(), InputError> {
        if n == 0 {
            return Ok(());
        }
        self.ensure_conn()?;
        let g = self.conn.lock();
        let c = g
            .as_ref()
            .ok_or_else(|| InputError::Os("x11 connection not initialised".into()))?;
        for _ in 0..n {
            tap(c, &self.emitted, EV_BACKSPACE)?;
        }
        Ok(())
    }

    fn send_keys(&self, keys: &[ReplayKey]) -> Result<(), InputError> {
        if keys.is_empty() {
            return Ok(());
        }
        debug!(count = keys.len(), "x11 replay starting");
        // Let the freshly-locked XKB group reach the focused client
        // before we replay against it — see `GROUP_SETTLE`.
        thread::sleep(GROUP_SETTLE);

        self.ensure_conn()?;
        let g = self.conn.lock();
        let c = g
            .as_ref()
            .ok_or_else(|| InputError::Os("x11 connection not initialised".into()))?;

        let last_idx = keys.len() - 1;
        for (i, rk) in keys.iter().enumerate() {
            let is_last = i == last_idx;
            debug!(scancode = rk.scancode, shift = rk.shift, "x11 key");
            if rk.shift {
                press(c, &self.emitted, EV_LEFTSHIFT)?;
                thread::sleep(KEY_STEP);
            }
            // The last key of a replay is the boundary the user typed —
            // usually Space, and the key whose *press* triggered this
            // correction milliseconds ago. Their finger is almost
            // certainly still on it. Releasing it first guarantees the
            // press that follows is a real down edge (and is a harmless
            // no-op if they have already let go); without it, servers
            // that collapse a press on an already-held key swallow the
            // boundary character and the corrected words run together.
            if is_last {
                release(c, &self.emitted, rk.scancode)?;
                thread::sleep(KEY_STEP);
            }
            tap(c, &self.emitted, rk.scancode)?;
            if rk.shift {
                release(c, &self.emitted, EV_LEFTSHIFT)?;
                thread::sleep(KEY_STEP);
            }
        }
        Ok(())
    }

    fn send_text(&self, text: &str) -> Result<(), InputError> {
        if text.is_empty() {
            return Ok(());
        }
        self.ensure_conn()?;
        let g = self.conn.lock();
        let c = g
            .as_ref()
            .ok_or_else(|| InputError::Os("x11 connection not initialised".into()))?;

        // Borrow one unbound keycode and re-point it at each character
        // in turn. This is how `xdotool type` works, and unlike the
        // GTK/Qt "Ctrl+Shift+U <hex> Space" compose dance the uinput
        // backend has to fall back on, it produces the character in
        // terminals too.
        let (scratch, per) = find_spare_keycode(&c.conn)?;
        let scratch_evdev = x11_to_evdev(u32::from(scratch))
            .ok_or_else(|| InputError::Os("spare keycode below the evdev offset".into()))?;
        debug!(scratch, chars = text.chars().count(), "x11 unicode type");

        let mut result = Ok(());
        for ch in text.chars() {
            result = bind_keysym(c, scratch, per, unicode_to_keysym(ch))
                .and_then(|()| tap(c, &self.emitted, scratch_evdev));
            if result.is_err() {
                break;
            }
        }
        // Put the keymap back even if a character failed halfway —
        // leaving a live binding on a borrowed keycode would corrupt
        // the user's keyboard for the rest of the session.
        let restored = unbind_keysym(c, scratch, per);
        result.and(restored)
    }

    fn take_emitted(&self) -> Vec<EmittedKey> {
        std::mem::take(&mut *self.emitted.lock())
    }

    fn backend_name(&self) -> &'static str {
        "linux-x11-xtest"
    }
}
