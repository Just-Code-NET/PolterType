//! Windows layout switcher.
//!
//! * Querying: `GetForegroundWindow` → `GetWindowThreadProcessId` →
//!   `GetKeyboardLayout(thread_id)`.
//! * Listing: `GetKeyboardLayoutList`.
//! * Switching: `PostMessageW(hwnd, WM_INPUTLANGCHANGEREQUEST, 0, hkl)`,
//!   not `ActivateKeyboardLayout`, which is per-thread and affects only
//!   the caller.
//! * Describing: `MapVirtualKeyExW` + `ToUnicodeEx` against each active
//!   HKL. The low word of an HKL says which *language* a keyboard is
//!   for; only the keys themselves say which *keyboard* it is.
//!
//! HKL → BCP-47 goes through `LCIDToLocaleName` on that low word.

use std::collections::HashMap;
use std::ffi::c_void;

use poltertype_types::OsKeymap;
use tracing::{debug, warn};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::Globalization::LCIDToLocaleName;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayout, GetKeyboardLayoutList, HKL, MAPVK_VSC_TO_VK_EX, MapVirtualKeyExW,
    ToUnicodeEx, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, PostMessageW, WM_INPUTLANGCHANGEREQUEST,
};

use crate::{LayoutError, LayoutId, LayoutSwitcher};

/// The character block we ask every keyboard about: number row, the
/// three letter rows, the backtick and backslash keys that move between
/// ANSI and ISO boards, and the extra key beside left Shift that only
/// ISO boards carry.
///
/// Deliberately a fixed list rather than one derived from the loaded
/// mappings: [`LayoutSwitcher::describe_keymaps`] promises a *complete*
/// table, and completeness only means something against a known set of
/// questions. A scancode asked about and absent from the answer is
/// evidence the key produces nothing — which is what lets the layout DB
/// replace a stale mapping rather than merge into it.
const CHARACTER_SCANCODES: &[u32] = &[
    // Number row: `1` … `=`
    0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, //
    // Q row: `Q` … `]`
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B, //
    // A row: `A` … `'`
    0x1E, 0x1F, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, //
    // Backtick and backslash
    0x29, 0x2B, //
    // Z row: `Z` … `/`
    0x2C, 0x2D, 0x2E, 0x2F, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35, //
    // The extra ISO key
    0x56,
];

/// `ToUnicodeEx` `wFlags` bit 2: "do not change keyboard state".
/// Windows 10 1607 and newer honour it; older builds ignore unknown
/// bits, which is why [`char_at`] drains dead keys by hand as well.
const TOUNICODE_NO_STATE_CHANGE: u32 = 0x4;

pub struct WindowsLayoutSwitcher;

impl WindowsLayoutSwitcher {
    pub fn new() -> Self {
        Self
    }
}

impl LayoutSwitcher for WindowsLayoutSwitcher {
    fn current(&self) -> Result<LayoutId, LayoutError> {
        // Safety: pure Win32 calls with no aliasing concerns.
        let hkl = unsafe {
            let hwnd = GetForegroundWindow();
            let tid = GetWindowThreadProcessId(hwnd, None);
            GetKeyboardLayout(tid)
        };
        Ok(hkl_to_layout_id(hkl))
    }

    fn list_active(&self) -> Result<Vec<LayoutId>, LayoutError> {
        // Safety: per-docs, calling with `None` returns the count.
        let count = unsafe { GetKeyboardLayoutList(None) } as usize;
        if count == 0 {
            return Ok(Vec::new());
        }
        let mut buf: Vec<HKL> = vec![HKL(std::ptr::null_mut::<c_void>()); count];
        // Safety: buffer length matches the count we just queried.
        let filled = unsafe { GetKeyboardLayoutList(Some(buf.as_mut_slice())) } as usize;
        buf.truncate(filled);
        Ok(buf.into_iter().map(hkl_to_layout_id).collect())
    }

    fn switch_to(&self, id: &LayoutId) -> Result<(), LayoutError> {
        let active = self.list_active()?;
        let active_map: HashMap<LayoutId, HKL> = self
            .raw_active()?
            .into_iter()
            .map(|h| (hkl_to_layout_id(h), h))
            .collect();

        let Some(&hkl) = active_map.get(id) else {
            warn!(?id, ?active, "requested layout is not active");
            return Err(LayoutError::NotActive(id.clone()));
        };

        // Safety: standard Win32 message-post.
        let res = unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_invalid() {
                return Err(LayoutError::Os("no foreground window".into()));
            }
            PostMessageW(
                hwnd,
                WM_INPUTLANGCHANGEREQUEST,
                WPARAM(0),
                LPARAM(hkl.0 as isize),
            )
        };
        res.map_err(|e| LayoutError::Os(format!("PostMessageW: {e}")))?;
        debug!(?id, ?hkl, "WM_INPUTLANGCHANGEREQUEST posted");
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "windows-hkl"
    }

    fn describe_keymaps(&self) -> Result<Vec<OsKeymap>, LayoutError> {
        let mut hkls = self.raw_active()?;

        // The keyboard in effect right now goes first. When a user has
        // installed two keyboards for one language both collapse to the
        // same `LayoutId` and the DB can only keep one of them — the one
        // they are typing on is by far the better guess.
        // Safety: pure Win32 calls with no aliasing concerns.
        let current = unsafe {
            let hwnd = GetForegroundWindow();
            let tid = GetWindowThreadProcessId(hwnd, None);
            GetKeyboardLayout(tid)
        };
        if let Some(pos) = hkls.iter().position(|h| h.0 == current.0) {
            hkls.swap(0, pos);
        }

        Ok(hkls.into_iter().map(describe_hkl).collect())
    }
}

impl WindowsLayoutSwitcher {
    fn raw_active(&self) -> Result<Vec<HKL>, LayoutError> {
        let count = unsafe { GetKeyboardLayoutList(None) } as usize;
        if count == 0 {
            return Ok(Vec::new());
        }
        let mut buf: Vec<HKL> = vec![HKL(std::ptr::null_mut::<c_void>()); count];
        let filled = unsafe { GetKeyboardLayoutList(Some(buf.as_mut_slice())) } as usize;
        buf.truncate(filled);
        Ok(buf)
    }
}

/// Convert a Windows `HKL` into a BCP-47 tag (`en-US`, `uk-UA`, …).
///
/// The low word of the HKL holds the input-language LCID. We resolve
/// it via `LCIDToLocaleName`. If anything goes wrong we fall back to
/// the hex representation so the user still sees a stable identifier.
fn hkl_to_layout_id(hkl: HKL) -> LayoutId {
    let raw = hkl.0 as usize as u32;
    let lcid = raw & 0xFFFF;

    let mut buf = [0u16; 85]; // LOCALE_NAME_MAX_LENGTH
    // Safety: standard Win32 call; we own the buffer.
    let written = unsafe { LCIDToLocaleName(lcid, Some(&mut buf), 0) };
    if written > 0 {
        let n = (written as usize).saturating_sub(1); // strip trailing NUL
        let s = String::from_utf16_lossy(&buf[..n]);
        if !s.is_empty() {
            return LayoutId::new(s);
        }
    }
    LayoutId::new(format!("hkl:{raw:08x}"))
}

/// Describe one keyboard: what each key of the character block
/// produces, unshifted and shifted.
///
/// `variant` is the raw HKL in hex, which for a standard keyboard is
/// exactly its KLID (`00030402` = Bulgarian Phonetic Traditional).
/// Imported and third-party layouts get a synthesised high word
/// instead, so it is only ever logged — never matched on.
fn describe_hkl(hkl: HKL) -> OsKeymap {
    let raw = hkl.0 as usize as u32;
    let variant = format!("{raw:08x}");
    let mut keys = Vec::with_capacity(CHARACTER_SCANCODES.len());

    for &sc in CHARACTER_SCANCODES {
        // `MAPVK_VSC_TO_VK_EX` resolves the scancode through *this*
        // layout. Zero means the keyboard has nothing on that key.
        // Safety: pure Win32 call against a handle the OS gave us.
        let vk = unsafe { MapVirtualKeyExW(sc, MAPVK_VSC_TO_VK_EX, hkl) };
        if vk == 0 {
            continue;
        }
        let Some(plain) = char_at(vk, sc, hkl, false) else {
            continue;
        };
        // `None` when Shift changes nothing — same convention the
        // bundled TOMLs use for their optional `shift` field.
        let shift = char_at(vk, sc, hkl, true).filter(|c| *c != plain);
        keys.push((sc, plain, shift));
    }

    let id = hkl_to_layout_id(hkl);
    debug!(%id, %variant, keys = keys.len(), "described keyboard");
    OsKeymap { id, variant, keys }
}

/// What does this key produce under `hkl`, with or without Shift?
///
/// Two `ToUnicodeEx` quirks shape this function. **Dead keys return
/// `-1`** and still write their character; reading that as "no
/// character" loses every accented key, and accounted for 22 of the 35
/// apparent mismatches in the audit behind issue #20. **It mutates
/// keyboard state**, so a pending dead key would compose itself into
/// whatever the *user* types next — hence
/// [`TOUNICODE_NO_STATE_CHANGE`], plus draining by hand because older
/// builds ignore the flag.
fn char_at(vk: u32, sc: u32, hkl: HKL, shift: bool) -> Option<char> {
    let mut state = [0u8; 256];
    if shift {
        state[VK_SHIFT.0 as usize] = 0x80;
    }

    let mut buf = [0u16; 8];
    // Safety: both buffers are ours and correctly sized; the call
    // cannot outlive them.
    let n = unsafe { ToUnicodeEx(vk, sc, &state, &mut buf, TOUNICODE_NO_STATE_CHANGE, hkl) };
    if n < 0 {
        // Dead key — feed it the same keystroke again so the pending
        // composition resolves and nothing is left waiting.
        let mut scratch = [0u16; 8];
        // Safety: both buffers are ours and correctly sized.
        unsafe { ToUnicodeEx(vk, sc, &state, &mut scratch, TOUNICODE_NO_STATE_CHANGE, hkl) };
    }
    if n == 0 {
        return None;
    }

    let len = if n < 0 {
        1
    } else {
        (n as usize).min(buf.len())
    };
    String::from_utf16_lossy(&buf[..len])
        .chars()
        .next()
        .filter(|c| !c.is_control())
}

#[cfg(test)]
mod tests;
