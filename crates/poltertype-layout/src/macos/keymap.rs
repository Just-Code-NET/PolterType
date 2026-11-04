//! Describing keyboards: what each key of the character block produces
//! under one TIS source, read with `UCKeyTranslate` against the source's
//! `kTISPropertyUnicodeKeyLayoutData` — the macOS counterpart of the
//! Windows backend's `ToUnicodeEx` walk.
//!
//! A language is not a keyboard: Apple ships `Arabic`, `Arabic - PC` and
//! `Arabic - North Africa`, which disagree on most of the bottom row, and
//! every one of them reports the same `LayoutId`. Asking the OS is the
//! only way the DB learns which one the user actually has.

use core_foundation::base::{CFRelease, TCFType};
use core_foundation::data::{CFData, CFDataRef};
use core_foundation::string::CFStringRef;
use poltertype_types::OsKeymap;

use super::consts::CHARACTER_KEYS;
use super::ids::{source_id_string, source_id_to_layout_id};
use super::{
    TISCopyCurrentKeyboardInputSource, TISGetInputSourceProperty, TISInputSourceRef,
    selectable_keyboards,
};
use crate::LayoutError;

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    static kTISPropertyUnicodeKeyLayoutData: CFStringRef;
    fn UCKeyTranslate(
        keyLayoutPtr: *const u8,
        virtualKeyCode: u16,
        keyAction: u16,
        modifierKeyState: u32,
        keyboardType: u32,
        keyTranslateOptions: u32,
        deadKeyState: *mut u32,
        maxStringLength: usize,
        actualStringLength: *mut usize,
        unicodeString: *mut u16,
    ) -> i32;
    fn LMGetKbdType() -> u8;
}

/// `kUCKeyActionDown`.
const KEY_ACTION_DOWN: u16 = 0;
/// `shiftKey >> 8`, the form `UCKeyTranslate` takes modifiers in.
const SHIFT_STATE: u32 = 0x02;
/// `kUCKeyTranslateNoDeadKeysMask`: a dead key reports its own character
/// instead of arming a compose state, which is what a key table wants.
const NO_DEAD_KEYS: u32 = 1;
/// Arabic tatweel, the kashida stretching mark.
const TATWEEL: char = '\u{0640}';

/// Describe one input source, or `None` when it carries no Unicode key
/// layout (input methods, palettes) and so is not a keyboard to describe.
///
/// # Safety
/// `src` must be a live `TISInputSourceRef`.
pub(super) unsafe fn describe_source(src: TISInputSourceRef) -> Option<OsKeymap> {
    let id = unsafe { source_id_to_layout_id(src) }?;
    let variant = unsafe { source_id_string(src) }.unwrap_or_default();
    // Get-rule: the property is owned by the source, not by us.
    let data_ref =
        unsafe { TISGetInputSourceProperty(src, kTISPropertyUnicodeKeyLayoutData) } as CFDataRef;
    if data_ref.is_null() {
        return None;
    }
    let data = unsafe { CFData::wrap_under_get_rule(data_ref) };
    let layout = data.bytes().as_ptr();
    let kbd_type = u32::from(unsafe { LMGetKbdType() });

    let mut keys = Vec::with_capacity(CHARACTER_KEYS.len());
    for &(sc, kvk) in CHARACTER_KEYS {
        let Some(plain) = (unsafe { char_at(layout, kvk, 0, kbd_type) }) else {
            continue;
        };
        // `None` when Shift changes nothing — same convention the bundled
        // TOMLs use for their optional `shift` field.
        let shift = unsafe { char_at(layout, kvk, SHIFT_STATE, kbd_type) }.filter(|c| *c != plain);
        keys.push((sc, plain, shift));
    }
    tracing::debug!(%id, %variant, keys = keys.len(), "described keyboard");
    Some(OsKeymap { id, variant, keys })
}

/// The single printable character a key produces, or `None` for nothing,
/// a control character, or any other multi-character output — a key
/// table entry is one `char`. Arabic's lam-alef keys type two code points
/// and are folded to their presentation form, as `ar.toml` does.
///
/// Tatweel (U+0640) is `None` too, as it is in `ar.toml`: it is
/// alphabetic but stretches a word rather than spelling one, and Apple's
/// Arabic keyboards put it on Shift+`-`, where it would make `_` read as
/// a letter and switch off the snake_case heuristic.
///
/// # Safety
/// `layout` must point at a live `UCKeyboardLayout`.
unsafe fn char_at(layout: *const u8, kvk: u16, modifiers: u32, kbd_type: u32) -> Option<char> {
    let mut dead_state = 0u32;
    let mut len = 0usize;
    let mut buf = [0u16; 4];
    let status = unsafe {
        UCKeyTranslate(
            layout,
            kvk,
            KEY_ACTION_DOWN,
            modifiers,
            kbd_type,
            NO_DEAD_KEYS,
            &mut dead_state,
            buf.len(),
            &mut len,
            buf.as_mut_ptr(),
        )
    };
    if status != 0 || len == 0 {
        return None;
    }
    if let Some(ligature) = lam_alef(&buf[..len]) {
        return Some(ligature);
    }
    let mut chars = char::decode_utf16(buf[..len].iter().copied());
    let c = chars.next()?.ok()?;
    if chars.next().is_some() || c.is_control() || c == TATWEEL {
        return None;
    }
    Some(c)
}

/// Every selectable keyboard, described; the one in effect comes first.
pub(super) fn describe_all() -> Result<Vec<OsKeymap>, LayoutError> {
    let Some(arr) = selectable_keyboards() else {
        return Ok(Vec::new());
    };
    // Safety: a copied source is retained and released right after its
    // id is read; the sources in `arr` live as long as `arr`.
    let current = unsafe {
        let src = TISCopyCurrentKeyboardInputSource();
        if src.is_null() {
            None
        } else {
            let id = source_id_string(src);
            CFRelease(src);
            id
        }
    };
    let mut out: Vec<OsKeymap> = arr
        .iter()
        .filter_map(|src| unsafe { describe_source(*src) })
        .collect();

    // The keyboard in effect right now goes first: two keyboards of one
    // language collapse to one `LayoutId`, and the DB keeps the first.
    if let Some(pos) = out
        .iter()
        .position(|k| Some(&k.variant) == current.as_ref())
    {
        out.swap(0, pos);
    }
    Ok(out)
}

/// The single-code-point presentation form of a lam-alef pair, so the
/// `B` key of an Arabic keyboard keeps its entry instead of being dropped.
fn lam_alef(utf16: &[u16]) -> Option<char> {
    match utf16 {
        [0x0644, 0x0627] => Some('\u{FEFB}'),
        [0x0644, 0x0623] => Some('\u{FEF7}'),
        [0x0644, 0x0625] => Some('\u{FEF9}'),
        [0x0644, 0x0622] => Some('\u{FEF5}'),
        _ => None,
    }
}
