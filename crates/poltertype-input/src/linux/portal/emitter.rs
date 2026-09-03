//! `KeyEmitter` over a RemoteDesktop portal session.

use std::sync::Mutex;

use tracing::warn;

use super::enums::PortalError;
use super::session::PortalSession;
use crate::{EmittedKey, InputError, KeyDirection, KeyEmitter, Modifiers, ReplayKey};

/// Evdev keycodes this backend needs by name. Same numbering the
/// portal takes and the same the rest of the crate uses — no X11 `+8`
/// anywhere in this file.
const KEY_BACKSPACE: i32 = 14;
const KEY_LEFTCTRL: i32 = 29;
const KEY_LEFTSHIFT: i32 = 42;
const KEY_LEFTALT: i32 = 56;
const KEY_LEFTMETA: i32 = 125;

pub struct PortalEmitter {
    session: PortalSession,
    /// Everything we have put on the wire since the engine last
    /// drained it. The portal's keys arrive back through evdev
    /// indistinguishable from a real keyboard — exactly the uinput
    /// situation — so the engine's echo filter needs the same record.
    emitted: Mutex<Vec<EmittedKey>>,
}

impl PortalEmitter {
    /// Negotiate a session. Blocks and may prompt, so this belongs at
    /// startup and never on the correction path.
    pub fn try_new() -> Result<Self, PortalError> {
        Ok(Self {
            session: PortalSession::open()?,
            emitted: Mutex::new(Vec::new()),
        })
    }

    fn tap(&self, keycode: i32) -> Result<(), InputError> {
        self.press(keycode, true)?;
        self.press(keycode, false)
    }

    fn press(&self, keycode: i32, down: bool) -> Result<(), InputError> {
        self.session
            .notify_keycode(keycode, down)
            .map_err(|e| InputError::Os(e.to_string()))?;
        // Both edges are recorded: the echo filter matches the stream
        // it actually sees, and evdev reports releases too.
        if let Ok(mut log) = self.emitted.lock() {
            log.push(EmittedKey {
                scancode: evdev_to_scancode(keycode),
                direction: if down {
                    KeyDirection::Press
                } else {
                    KeyDirection::Release
                },
            });
        }
        Ok(())
    }
}

impl KeyEmitter for PortalEmitter {
    fn send_backspaces(&self, n: usize) -> Result<(), InputError> {
        for _ in 0..n {
            self.tap(KEY_BACKSPACE)?;
        }
        Ok(())
    }

    /// The portal speaks keycodes, not characters.
    ///
    /// Not a gap: the Wayland correction path is scancode replay for
    /// the same reason the uinput backend uses it — the compose-key
    /// trick `send_text` would need is swallowed by most terminals
    /// and Wayland-native apps. Returning `Unsupported` is what tells
    /// the engine to use [`send_keys`](Self::send_keys).
    fn send_text(&self, _text: &str) -> Result<(), InputError> {
        Err(InputError::Unsupported(
            "the RemoteDesktop portal emits keycodes, not text; use send_keys".into(),
        ))
    }

    fn send_keys(&self, keys: &[ReplayKey]) -> Result<(), InputError> {
        for key in keys {
            let code = scancode_to_evdev(key.scancode);
            if key.shift {
                self.press(KEY_LEFTSHIFT, true)?;
            }
            self.tap(code)?;
            if key.shift {
                self.press(KEY_LEFTSHIFT, false)?;
            }
        }
        Ok(())
    }

    /// Let go of modifiers the user is physically holding.
    ///
    /// Same reasoning as every other backend: our keys travel the
    /// user's path to the application, so a held `Ctrl` turns the
    /// replay into shortcuts and types nothing. We release and never
    /// re-press — re-pressing one the user has meanwhile let go of
    /// would leave it stuck down.
    fn release_modifiers(&self, held: Modifiers) -> Result<(), InputError> {
        for (is_held, code) in [
            (held.control, KEY_LEFTCTRL),
            (held.alt, KEY_LEFTALT),
            (held.shift, KEY_LEFTSHIFT),
            (held.meta, KEY_LEFTMETA),
        ] {
            if is_held && let Err(e) = self.press(code, false) {
                warn!(?e, code, "portal: could not release a held modifier");
            }
        }
        Ok(())
    }

    fn take_emitted(&self) -> Vec<EmittedKey> {
        self.emitted
            .lock()
            .map(|mut log| std::mem::take(&mut *log))
            .unwrap_or_default()
    }

    fn backend_name(&self) -> &'static str {
        "linux-portal-remotedesktop"
    }
}

/// Win SC Set-1 scancode → evdev keycode.
///
/// The engine's buffers carry Set-1 scancodes (what the layout TOMLs
/// are keyed by); evdev codes are what both the portal and
/// `/dev/input` speak. For the alphanumeric block the two coincide,
/// which is why this is an identity — the block PolterType records is
/// exactly the block where Set-1 and evdev agree.
fn scancode_to_evdev(scancode: u32) -> i32 {
    i32::try_from(scancode).unwrap_or(0)
}

fn evdev_to_scancode(keycode: i32) -> u32 {
    u32::try_from(keycode).unwrap_or(0)
}

/// Test-only views of the private constants and conversions, so the
/// tests pin what the code actually uses rather than a copy of it.
#[cfg(test)]
pub(super) mod testing {
    pub fn key_backspace() -> i32 {
        super::KEY_BACKSPACE
    }
    pub fn key_leftshift() -> i32 {
        super::KEY_LEFTSHIFT
    }
    pub fn key_leftctrl() -> i32 {
        super::KEY_LEFTCTRL
    }
    pub fn key_leftalt() -> i32 {
        super::KEY_LEFTALT
    }
    pub fn key_leftmeta() -> i32 {
        super::KEY_LEFTMETA
    }
    pub fn to_evdev(scancode: u32) -> i32 {
        super::scancode_to_evdev(scancode)
    }
    pub fn from_evdev(keycode: i32) -> u32 {
        super::evdev_to_scancode(keycode)
    }
}
