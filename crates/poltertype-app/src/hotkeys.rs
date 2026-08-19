//! Hotkey string parsing and scancode mapping.

use global_hotkey::hotkey::{Code, HotKey, Modifiers as HkMods};
use tracing::warn;

/// Parse a `[hotkeys]` string, falling back to `default_str` on a bad
/// value so a typo cannot silently cost the user their hotkeys.
pub(crate) fn parse_hotkey_or_default(s: &str, default_str: &str) -> HotKey {
    match s.parse::<HotKey>() {
        Ok(h) => h,
        Err(e) => {
            warn!(
                ?e,
                raw = s,
                fallback = default_str,
                "could not parse hotkey; using fallback"
            );
            // The fallback is itself a parse; the hard-coded combo is
            // the last resort, so a real hotkey always comes back.
            default_str
                .parse::<HotKey>()
                .unwrap_or_else(|_| HotKey::new(Some(HkMods::CONTROL | HkMods::SHIFT), Code::Space))
        }
    }
}

/// Resolve a parsed [`HotKey`] into an engine-side [`Chord`] for the
/// keystream (Wayland) path. Returns `None` when the main key has no
/// entry in our SC Set-1 table — the chord is then simply unbound on
/// that backend (best-effort, per the Wayland support policy).
pub(crate) fn chord_from_hotkey(hk: &HotKey) -> Option<poltertype_core::engine::Chord> {
    Some(poltertype_core::engine::Chord {
        ctrl: hk.mods.contains(HkMods::CONTROL),
        shift: hk.mods.contains(HkMods::SHIFT),
        alt: hk.mods.contains(HkMods::ALT),
        meta: hk.mods.contains(HkMods::META),
        scancode: code_to_sc1(hk.key)?,
    })
}

/// W3C `Code` → Win SC Set-1 scancode. On Linux these coincide with the
/// evdev key codes the listener reports (see `evdev_to_sc1`), so the
/// same table serves matching against the live stream. Covers the keys
/// realistically used in a hotkey; anything else returns `None`.
pub(crate) fn code_to_sc1(code: Code) -> Option<u32> {
    Some(match code {
        Code::Escape => 0x01,
        Code::Digit1 => 0x02,
        Code::Digit2 => 0x03,
        Code::Digit3 => 0x04,
        Code::Digit4 => 0x05,
        Code::Digit5 => 0x06,
        Code::Digit6 => 0x07,
        Code::Digit7 => 0x08,
        Code::Digit8 => 0x09,
        Code::Digit9 => 0x0A,
        Code::Digit0 => 0x0B,
        Code::Minus => 0x0C,
        Code::Equal => 0x0D,
        Code::Backspace => 0x0E,
        Code::Tab => 0x0F,
        Code::KeyQ => 0x10,
        Code::KeyW => 0x11,
        Code::KeyE => 0x12,
        Code::KeyR => 0x13,
        Code::KeyT => 0x14,
        Code::KeyY => 0x15,
        Code::KeyU => 0x16,
        Code::KeyI => 0x17,
        Code::KeyO => 0x18,
        Code::KeyP => 0x19,
        Code::Enter => 0x1C,
        Code::KeyA => 0x1E,
        Code::KeyS => 0x1F,
        Code::KeyD => 0x20,
        Code::KeyF => 0x21,
        Code::KeyG => 0x22,
        Code::KeyH => 0x23,
        Code::KeyJ => 0x24,
        Code::KeyK => 0x25,
        Code::KeyL => 0x26,
        Code::KeyZ => 0x2C,
        Code::KeyX => 0x2D,
        Code::KeyC => 0x2E,
        Code::KeyV => 0x2F,
        Code::KeyB => 0x30,
        Code::KeyN => 0x31,
        Code::KeyM => 0x32,
        Code::Space => 0x39,
        Code::F1 => 0x3B,
        Code::F2 => 0x3C,
        Code::F3 => 0x3D,
        Code::F4 => 0x3E,
        Code::F5 => 0x3F,
        Code::F6 => 0x40,
        Code::F7 => 0x41,
        Code::F8 => 0x42,
        Code::F9 => 0x43,
        Code::F10 => 0x44,
        Code::F11 => 0x57,
        Code::F12 => 0x58,
        _ => return None,
    })
}
