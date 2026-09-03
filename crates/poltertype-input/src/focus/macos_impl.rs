//! macOS focus tracking: Accessibility (HIServices) queries.
//!
//! Two hardware findings shape this; the write-up is in
//! `docs/MACOS_POPUP.md` and the 2026-08-11 entry of
//! `docs/DECISIONS.md`:
//!
//! * `AXUIElementCreateSystemWide` — the textbook route — answers
//!   `kAXErrorCannotComplete` essentially always (macOS 15.7, Intel),
//!   so every query hangs off an app element built from the frontmost
//!   pid instead.
//! * Chrome (omnibox *and* web inputs) and Terminal.app answer the
//!   caret question with junk and no error, so caret bounds are
//!   validated before they are trusted.
//!
//! A denied or failed query maps to `None` and the caller degrades to
//! a coarser anchor. The FFI is declared by hand because the
//! `core-foundation` crate stops at CF and does not wrap HIServices.

use std::ffi::CStr;
use std::ffi::c_void;
use std::path::Path;
use std::time::Duration;

use core_foundation::base::CFTypeRef;
use core_foundation::string::CFStringRef;
use core_graphics::geometry::{CGPoint, CGRect, CGSize};
use objc2_app_kit::NSWorkspace;

use super::traits::FocusTracker;
use super::types::{CaretHint, FocusedWindowGeometry};

mod ax;
mod consts;
mod types;

use ax::{
    ax_value, copy_attr, copy_attr_retry, copy_string_attr, element_frame, parameterized_rect,
};
use consts::{
    AX_MSG_TIMEOUT_SECS, CARET_FRAME_SLACK, K_AXVALUE_TYPE_CFRANGE, MAX_CARET_HEIGHT,
    MIN_CARET_HEIGHT,
};
use types::{CFRange, OwnedCF};

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXUIElementCreateApplication(pid: libc::pid_t) -> CFTypeRef;
    fn AXUIElementCopyAttributeValue(
        element: CFTypeRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> i32;
    fn AXUIElementCopyParameterizedAttributeValue(
        element: CFTypeRef,
        attribute: CFStringRef,
        parameter: CFTypeRef,
        value: *mut CFTypeRef,
    ) -> i32;
    fn AXUIElementSetMessagingTimeout(element: CFTypeRef, seconds: f32) -> i32;
    fn AXValueCreate(value_type: u32, value_ptr: *const c_void) -> CFTypeRef;
    fn AXValueGetValue(value: CFTypeRef, value_type: u32, value_ptr: *mut c_void) -> bool;
}

pub struct MacosFocusTracker;

impl MacosFocusTracker {
    /// The frontmost application's pid — `NSWorkspace` answers this
    /// without any AX traffic at all.
    fn frontmost_pid() -> Option<libc::pid_t> {
        let pid = NSWorkspace::sharedWorkspace()
            .frontmostApplication()?
            .processIdentifier();
        if pid > 0 { Some(pid) } else { None }
    }

    /// The frontmost app's AX element, with the messaging timeout
    /// already clamped — every query below goes through it.
    fn app_element() -> Option<OwnedCF> {
        let pid = Self::frontmost_pid()?;
        // Safety: creating a reference for a live pid. The result is
        // non-null even for apps that later refuse to answer; those
        // fail at query time, not here.
        let element = unsafe { AXUIElementCreateApplication(pid) };
        if element.is_null() {
            return None;
        }
        let owned = OwnedCF(element);
        // Safety: live element; a failure only means queries keep the
        // default (long) timeout — not worth failing the tracker over.
        unsafe { AXUIElementSetMessagingTimeout(owned.0, AX_MSG_TIMEOUT_SECS) };
        Some(owned)
    }

    fn focused_window_rect() -> Option<CGRect> {
        let app = Self::app_element()?;
        let window = copy_attr(app.0, "AXFocusedWindow")?;
        element_frame(window.0)
    }

    /// What the caret APIs say, unvalidated. The marker-range pair
    /// (`AXSelectedTextMarkerRange` → `AXBoundsForTextMarkerRange`)
    /// goes first: WebKit implements it properly where the plain
    /// range pair answers junk. The plain pair is then tried with the
    /// selection as-is and with its length clamped to ≥ 1 — some apps
    /// only answer non-empty ranges.
    fn raw_caret_bounds(element: CFTypeRef) -> Option<CGRect> {
        if let Some(marker) = copy_attr(element, "AXSelectedTextMarkerRange") {
            if let Some(rect) = parameterized_rect(element, "AXBoundsForTextMarkerRange", marker.0)
            {
                return Some(rect);
            }
        }
        let range_value = copy_attr(element, "AXSelectedTextRange")?;
        let range = ax_value::<CFRange>(range_value.0, K_AXVALUE_TYPE_CFRANGE)?;
        for candidate in [
            range,
            CFRange {
                location: range.location,
                length: range.length.max(1),
            },
        ] {
            // The answer comes back in global screen coordinates.
            // Safety: `candidate` outlives the Create call; Create
            // rule — the result is ours (OwnedCF).
            let param = unsafe {
                AXValueCreate(
                    K_AXVALUE_TYPE_CFRANGE,
                    std::ptr::from_ref(&candidate).cast::<c_void>(),
                )
            };
            if param.is_null() {
                continue;
            }
            let param = OwnedCF(param);
            if let Some(rect) = parameterized_rect(element, "AXBoundsForRange", param.0) {
                return Some(rect);
            }
        }
        None
    }

    /// Is `rect` a believable caret for an element whose frame is
    /// `frame`? A real caret is a thin sliver one line tall, in the
    /// neighbourhood of its element; junk anchors the tooltip to
    /// wherever the caret *previously* was, which reads to a user as
    /// flakiness rather than as a wrong answer.
    fn caret_is_sane(rect: CGRect, frame: Option<CGRect>) -> bool {
        let (w, h) = (rect.size.width, rect.size.height);
        if !rect.origin.x.is_finite()
            || !rect.origin.y.is_finite()
            || !w.is_finite()
            || !h.is_finite()
        {
            return false;
        }
        if !(MIN_CARET_HEIGHT..=MAX_CARET_HEIGHT).contains(&h) {
            return false;
        }
        if w < 0.0 || w > 12.0_f64.max(h * 1.5) {
            return false;
        }
        let Some(f) = frame else { return true };
        let near = CGRect::new(
            &CGPoint::new(
                f.origin.x - CARET_FRAME_SLACK,
                f.origin.y - CARET_FRAME_SLACK,
            ),
            &CGSize::new(
                f.size.width + 2.0 * CARET_FRAME_SLACK,
                f.size.height + 2.0 * CARET_FRAME_SLACK,
            ),
        );
        rect.is_intersects(&near)
    }

    /// Roles whose frame is a good tooltip anchor when the caret is
    /// unavailable or junk — text entry widgets. (The search field is
    /// an `AXTextField` subrole, the omnibox a plain `AXTextField`.)
    fn is_text_role(element: CFTypeRef) -> bool {
        matches!(
            copy_string_attr(element, "AXRole").as_deref(),
            Some("AXTextField") | Some("AXTextArea") | Some("AXComboBox")
        )
    }

    /// Global rect → window-relative hint.
    fn hint_from(rect: CGRect, window: CGRect) -> CaretHint {
        CaretHint {
            x: rect.origin.x as i32 - window.origin.x as i32,
            y: rect.origin.y as i32 - window.origin.y as i32,
            height: rect.size.height as u32,
            // A live query, not a cached sample — always fresh, and by
            // construction from the window it will be composed with.
            age: Duration::ZERO,
            pid: None,
            window: None,
        }
    }
}

impl FocusTracker for MacosFocusTracker {
    fn focused_exe(&self) -> Option<String> {
        let pid = Self::frontmost_pid()?;
        let mut buf = [0i8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        // Safety: `buf` is a valid writable buffer of the given size.
        let len = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
        if len <= 0 {
            return None;
        }
        let path = unsafe { CStr::from_ptr(buf.as_ptr()) }.to_string_lossy();
        let name = Path::new(path.as_ref())
            .file_name()?
            .to_string_lossy()
            .into_owned();
        Some(name)
    }

    fn focused_window_geometry(&self) -> Option<FocusedWindowGeometry> {
        let rect = Self::focused_window_rect()?;
        Some(FocusedWindowGeometry {
            x: rect.origin.x as i32,
            y: rect.origin.y as i32,
            width: rect.size.width as u32,
            height: rect.size.height as u32,
            // A caret hint here is read live off the frontmost
            // application's focused window, so it needs no proof of
            // ownership to be matched against.
            pid: None,
        })
    }

    fn caret_hint(&self) -> Option<CaretHint> {
        let app = Self::app_element()?;
        // The one query that races the target's own focus handling —
        // worth the retry budget.
        let element = copy_attr_retry(app.0, "AXFocusedUIElement")?;
        let window = Self::focused_window_rect()?;
        let frame = element_frame(element.0);

        if let Some(rect) = Self::raw_caret_bounds(element.0)
            && Self::caret_is_sane(rect, frame)
        {
            return Some(Self::hint_from(rect, window));
        }

        // The Chrome/Terminal path: the caret answer is junk but the
        // text widget's own frame is exact.
        if Self::is_text_role(element.0)
            && let Some(f) = frame
            && f.size.width > 1.0
            && f.size.height > 1.0
        {
            return Some(Self::hint_from(f, window));
        }

        None
    }

    fn backend_name(&self) -> &'static str {
        "macos-ax"
    }
}

pub(crate) fn create_macos_focus_tracker() -> std::sync::Arc<dyn FocusTracker> {
    std::sync::Arc::new(MacosFocusTracker)
}

#[cfg(test)]
mod tests;
