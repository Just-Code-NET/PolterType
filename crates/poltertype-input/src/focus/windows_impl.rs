//! Windows foreground-process query.
//!
//! `GetForegroundWindow` → `GetWindowThreadProcessId` →
//! `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` →
//! `QueryFullProcessImageNameW` → take the basename. Standard
//! sequence — needs no special permission and works for
//! cross-elevation foreground apps (LIMITED_INFORMATION is the
//! reduced-privilege variant designed exactly for this).

use std::path::Path;

use tracing::warn;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
use windows::core::PWSTR;

use super::FocusTracker;

pub struct WindowsFocusTracker;

impl FocusTracker for WindowsFocusTracker {
    fn focused_exe(&self) -> Option<String> {
        // Safety: a chain of standard Win32 calls; we close the
        // process handle exactly once before returning.
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.0.is_null() {
                return None;
            }
            let mut pid: u32 = 0;
            let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == 0 {
                return None;
            }
            let process = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                Ok(h) => h,
                Err(e) => {
                    warn!(?e, pid, "OpenProcess failed");
                    return None;
                }
            };
            let mut buf = [0u16; 1024];
            let mut len: u32 = buf.len() as u32;
            let q = QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                PWSTR(buf.as_mut_ptr()),
                &mut len,
            );
            let _ = CloseHandle(process);
            if let Err(e) = q {
                warn!(?e, pid, "QueryFullProcessImageNameW failed");
                return None;
            }
            let path = String::from_utf16_lossy(&buf[..len as usize]);
            let name = Path::new(&path).file_name()?.to_string_lossy().into_owned();
            Some(name)
        }
    }

    fn focused_window_geometry(&self) -> Option<crate::focus::FocusedWindowGeometry> {
        // Safety: `GetWindowRect` writes into the RECT we own; no
        // handles to release.
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.0.is_null() {
                return None;
            }
            let mut rect = windows::Win32::Foundation::RECT::default();
            if let Err(e) = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut rect)
            {
                warn!(?e, "GetWindowRect failed");
                return None;
            }
            let width = u32::try_from(rect.right.saturating_sub(rect.left)).ok()?;
            let height = u32::try_from(rect.bottom.saturating_sub(rect.top)).ok()?;
            Some(crate::focus::FocusedWindowGeometry {
                x: rect.left,
                y: rect.top,
                width,
                height,
                // Virtual-screen coordinates are already global on
                // Windows; no output mapping needed.
                output: None,
                output_x: 0,
                output_y: 0,
            })
        }
    }

    fn backend_name(&self) -> &'static str {
        "windows-foreground-process"
    }
}
