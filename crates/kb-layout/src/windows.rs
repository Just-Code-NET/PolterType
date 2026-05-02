//! Windows layout switcher.
//!
//! * Querying: `GetForegroundWindow` → `GetWindowThreadProcessId` →
//!   `GetKeyboardLayout(thread_id)`.
//! * Listing: `GetKeyboardLayoutList`.
//! * Switching: `PostMessageW(hwnd, WM_INPUTLANGCHANGEREQUEST, 0, hkl)` —
//!   correct way to ask the foreground window to change layout. We
//!   avoid `ActivateKeyboardLayout` because it is per-thread and only
//!   affects whoever calls it.
//!
//! HKL → BCP-47 mapping uses `LCIDToLocaleName` on the low word of
//! the HKL, which holds the input-language LCID.

use std::collections::HashMap;
use std::ffi::c_void;

use tracing::{debug, warn};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::Globalization::LCIDToLocaleName;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardLayout, GetKeyboardLayoutList, HKL};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, PostMessageW, WM_INPUTLANGCHANGEREQUEST,
};

use crate::{LayoutError, LayoutId, LayoutSwitcher};

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
