//! The Win32 window itself: create it, push pixels at it, hide it.
//!
//! Every call into Win32 in this backend is here, so the thread in
//! [`super::popup`] reads as plain Rust — the same split the X11
//! backend uses.
//!
//! **Layered rather than painted.** The tooltip has rounded corners and
//! a translucent panel over somebody else's text, and a plain window
//! has no per-pixel alpha, so `WM_PAINT` would give a rectangle.
//! `UpdateLayeredWindow` composites a 32-bit premultiplied BGRA surface
//! — exactly what [`crate::render`] already produces for Wayland, so
//! the pixels cross unchanged but for channel order. It also means
//! there is no `WM_PAINT` to answer, no flicker, and no repaint when
//! the window behind scrolls.

use tracing::warn;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC,
    MONITOR_DEFAULTTONEAREST, MonitorFromPoint, ReleaseDC, SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetSystemMetrics, HWND_TOPMOST, RegisterClassW,
    SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_HIDE,
    SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos, ShowWindow, ULW_ALPHA,
    UpdateLayeredWindow, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use super::consts::{BASE_DPI, CLASS_NAME};

/// A created, hidden, never-activated overlay window.
pub(super) struct PopupWindow {
    hwnd: HWND,
}

impl PopupWindow {
    /// Register the class (once) and create the window.
    ///
    /// The extended styles are the whole "never steal focus" guarantee:
    /// `WS_EX_NOACTIVATE` so clicking the tooltip does not deactivate
    /// the editor the user is typing into — the exact failure this
    /// crate exists to avoid; `WS_EX_TOPMOST` to sit above the focused
    /// window; `WS_EX_TOOLWINDOW` to stay out of the taskbar and
    /// Alt+Tab; `WS_EX_LAYERED` because `UpdateLayeredWindow` requires
    /// it.
    ///
    /// `WS_POPUP` rather than `WS_OVERLAPPED`, so there is no caption,
    /// border or system menu to draw or click.
    pub(super) fn create() -> Option<Self> {
        // Safety: GetModuleHandleW(None) returns this module's handle.
        let instance = unsafe { GetModuleHandleW(None) }.ok()?;

        let class = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            hInstance: instance.into(),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        // Registering twice returns 0; that is fine — the class from
        // the first popup is still there and still ours. Any other
        // failure shows up as a null window below.
        // Safety: `class` outlives the call; all pointers in it are
        // static or owned here.
        unsafe { RegisterClassW(&class) };

        // Safety: standard window creation. Size and position are set
        // for real by `show`, which is the only thing that knows how
        // big the rendered tooltip is.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TOPMOST
                    | WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE
                    // Clicks pass through until a model is shown; the
                    // style is cleared in `show` and restored in
                    // `hide`, so an invisible window is never in the
                    // way of the desktop underneath it.
                    | WS_EX_TRANSPARENT,
                CLASS_NAME,
                windows::core::w!(""),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                HINSTANCE::from(instance),
                None,
            )
        }
        .ok()?;

        Some(Self { hwnd })
    }

    /// The scale the renderer should draw at for a tooltip landing at
    /// `(x, y)` in virtual-screen coordinates.
    ///
    /// Per monitor rather than per system: a laptop panel at 150% beside
    /// an external display at 100% is the ordinary Windows desktop, and
    /// the wrong scale is either blurry or half-sized.
    pub(super) fn scale_at(x: i32, y: i32) -> f32 {
        let mut dpi_x = BASE_DPI;
        let mut dpi_y = BASE_DPI;
        // Safety: MonitorFromPoint always returns a monitor with
        // MONITOR_DEFAULTTONEAREST; GetDpiForMonitor writes two u32s.
        unsafe {
            let monitor = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
            let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
        }
        dpi_x as f32 / BASE_DPI as f32
    }

    /// The whole virtual desktop, in the coordinates `show` takes.
    /// Used as the placement bounds — the union of every monitor, so a
    /// tooltip near the edge of one screen slides along that edge
    /// rather than being clamped onto the primary one.
    pub(super) fn virtual_screen() -> (i32, i32, i32, i32) {
        // Safety: GetSystemMetrics is a pure query.
        unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN),
            )
        }
    }

    /// Put `rgba` (premultiplied, as `tiny_skia` produces) on screen at
    /// `(x, y)`, sizing the window to match, without activating it.
    ///
    /// `false` if the surface could not be handed to Windows, in which
    /// case nothing is shown and the caller reports no popup — better
    /// than a window sitting there with stale pixels.
    pub(super) fn show(&self, rgba: &[u8], w: i32, h: i32, x: i32, y: i32) -> bool {
        if w <= 0 || h <= 0 || rgba.len() < (w * h * 4) as usize {
            return false;
        }

        // Safety: every handle below is checked and released on every
        // path; the DIB is written only within its own allocation.
        unsafe {
            let screen_dc = GetDC(None);
            if screen_dc.is_invalid() {
                return false;
            }
            let mem_dc = CreateCompatibleDC(screen_dc);
            if mem_dc.is_invalid() {
                ReleaseDC(None, screen_dc);
                return false;
            }

            // Top-down (negative height) so row 0 is the top row, which
            // is how the renderer lays the pixmap out.
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };

            let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
            let bitmap = CreateDIBSection(mem_dc, &info, DIB_RGB_COLORS, &mut bits, None, 0);
            let Ok(bitmap) = bitmap else {
                let _ = DeleteDC(mem_dc);
                ReleaseDC(None, screen_dc);
                return false;
            };
            if bits.is_null() {
                let _ = DeleteObject(bitmap);
                let _ = DeleteDC(mem_dc);
                ReleaseDC(None, screen_dc);
                return false;
            }

            // RGBA → BGRA. Both are premultiplied already, which is
            // what `AC_SRC_ALPHA` below promises Windows, so this is a
            // channel swap and nothing more.
            let count = (w * h) as usize;
            let dst = std::slice::from_raw_parts_mut(bits.cast::<u8>(), count * 4);
            for i in 0..count {
                let s = i * 4;
                dst[s] = rgba[s + 2];
                dst[s + 1] = rgba[s + 1];
                dst[s + 2] = rgba[s];
                dst[s + 3] = rgba[s + 3];
            }

            let old = SelectObject(mem_dc, bitmap);

            let size = windows::Win32::Foundation::SIZE { cx: w, cy: h };
            let src = POINT { x: 0, y: 0 };
            let dest = POINT { x, y };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };

            let ok = UpdateLayeredWindow(
                self.hwnd,
                screen_dc,
                Some(&dest),
                Some(&size),
                mem_dc,
                Some(&src),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            )
            .is_ok();

            SelectObject(mem_dc, old);
            let _ = DeleteObject(bitmap);
            let _ = DeleteDC(mem_dc);
            ReleaseDC(None, screen_dc);

            if !ok {
                warn!("UpdateLayeredWindow failed; not showing the tooltip");
                return false;
            }

            // Clicks are for us now.
            self.set_click_through(false);
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            // Re-assert topmost: another app going full-screen can push
            // us down, and SWP_NOACTIVATE keeps this from stealing
            // focus the way a plain SetForegroundWindow would.
            let _ = SetWindowPos(
                self.hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
            true
        }
    }

    pub(super) fn hide(&self) {
        // Safety: hiding a window we own.
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
        // Back to pass-through so a hidden overlay can never eat a
        // click meant for the desktop.
        self.set_click_through(true);
    }

    /// Add or drop `WS_EX_TRANSPARENT`, which decides whether the
    /// window is hit-testable at all.
    fn set_click_through(&self, through: bool) {
        use windows::Win32::UI::WindowsAndMessaging::{
            GWL_EXSTYLE, GetWindowLongPtrW, SetWindowLongPtrW,
        };
        // Safety: reading and writing our own window's style bits.
        unsafe {
            let cur = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
            let bit = WS_EX_TRANSPARENT.0 as isize;
            let next = if through { cur | bit } else { cur & !bit };
            if next != cur {
                SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, next);
            }
        }
    }
}

impl Drop for PopupWindow {
    fn drop(&mut self) {
        // Safety: destroying a window we created, from the thread that
        // created it — which is the only thread that owns this type.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

/// Nothing to do here on purpose: the popup's own loop reads mouse
/// messages out of the queue with `PeekMessageW` before dispatching, so
/// hit-testing happens in plain Rust with the row rectangles in scope
/// rather than in a C callback needing state smuggled through
/// `GWLP_USERDATA`.
unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}
