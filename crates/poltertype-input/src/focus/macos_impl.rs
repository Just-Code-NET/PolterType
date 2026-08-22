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

use core_foundation::base::{CFGetTypeID, CFRelease, CFTypeRef, TCFType};
use core_foundation::string::{CFString, CFStringGetTypeID, CFStringRef};
use core_graphics::geometry::{CGPoint, CGRect, CGSize};
use objc2_app_kit::NSWorkspace;

use super::traits::FocusTracker;
use super::types::{CaretHint, FocusedWindowGeometry};

// AXValueType constants from HIServices/AXValue.h.
const K_AXVALUE_TYPE_CGPOINT: u32 = 1;
const K_AXVALUE_TYPE_CGSIZE: u32 = 2;
const K_AXVALUE_TYPE_CGRECT: u32 = 3;
const K_AXVALUE_TYPE_CFRANGE: u32 = 4;

/// Cap on how long one AX query may block the UI event loop. An app
/// that cannot answer within this is treated as one without a11y.
const AX_MSG_TIMEOUT_SECS: f32 = 0.3;

/// A caret with zero height is no caret — it is the empty junk rect
/// several apps hand back instead. The cap filters the other extreme
/// (a whole-line "selection bounds" answer).
const MIN_CARET_HEIGHT: f64 = 0.5;
const MAX_CARET_HEIGHT: f64 = 120.0;

/// Retry budget for the focused-element query: it can race the target
/// app's own focus-change handling and answer `cannotComplete` /
/// `noValue` transiently (SuperDictate's resolver does the same).
const FOCUS_RETRY_ATTEMPTS: usize = 3;
const FOCUS_RETRY_DELAY: Duration = Duration::from_millis(40);

/// Slack for the "caret belongs to its element" check: real carets
/// stick out of the field's frame by a few points (TextEdit's search
/// field reports one 9 pt above its frame), Chrome's junk is hundreds
/// of points away.
const CARET_FRAME_SLACK: f64 = 24.0;

/// `CFRange` from CFBase — declared locally rather than pulling in
/// `core-foundation-sys` for one struct.
#[repr(C)]
#[derive(Clone, Copy)]
struct CFRange {
    location: isize,
    length: isize,
}

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

/// An owned AX/CF reference — every successful `Copy`/`Create` call
/// above hands us a +1 object, and this is the single place it goes
/// back.
struct OwnedCF(CFTypeRef);

impl Drop for OwnedCF {
    fn drop(&mut self) {
        // Safety: balancing the +1 from the Copy/Create call this
        // wrapper was built from; never constructed from a null.
        unsafe { CFRelease(self.0) }
    }
}

/// Copy an attribute, or `None` on any AX error (no permission, no
/// focused element, app without a11y — all mean "degrade gracefully").
fn copy_attr(element: CFTypeRef, name: &'static str) -> Option<OwnedCF> {
    let attr = CFString::from_static_string(name);
    let mut value: CFTypeRef = std::ptr::null();
    // Safety: `element` is a live AX object, `value` is ours to fill.
    let err =
        unsafe { AXUIElementCopyAttributeValue(element, attr.as_concrete_TypeRef(), &mut value) };
    if err != 0 || value.is_null() {
        return None;
    }
    Some(OwnedCF(value))
}

fn copy_string_attr(element: CFTypeRef, name: &'static str) -> Option<String> {
    let value = copy_attr(element, name)?;
    // Safety: `value` is a live CF object we own; the type check keeps
    // a non-string answer from being read as one. Get-rule wrap: the
    // +1 stays with `value` (dropped below), the CFString wrapper
    // retains its own.
    unsafe {
        if CFGetTypeID(value.0) != CFStringGetTypeID() {
            return None;
        }
        Some(CFString::wrap_under_get_rule(value.0.cast()).to_string())
    }
}

fn ax_value<T: Copy>(value: CFTypeRef, value_type: u32) -> Option<T> {
    let mut out: T = unsafe { std::mem::zeroed() };
    // Safety: `out` is the exact payload type `value_type` promises to
    // write, and `value` is a live AXValue we own.
    let ok = unsafe { AXValueGetValue(value, value_type, std::ptr::from_mut(&mut out).cast()) };
    if ok { Some(out) } else { None }
}

/// A parameterized attribute whose answer is an AXValue-wrapped
/// CGRect (`kAXBoundsForRange`, `AXBoundsForTextMarkerRange`).
fn parameterized_rect(
    element: CFTypeRef,
    name: &'static str,
    parameter: CFTypeRef,
) -> Option<CGRect> {
    let attr = CFString::from_static_string(name);
    let mut bounds: CFTypeRef = std::ptr::null();
    // Safety: live element, live parameter, out-pointer is ours.
    let err = unsafe {
        AXUIElementCopyParameterizedAttributeValue(
            element,
            attr.as_concrete_TypeRef(),
            parameter,
            &mut bounds,
        )
    };
    if err != 0 || bounds.is_null() {
        return None;
    }
    let bounds = OwnedCF(bounds);
    ax_value::<CGRect>(bounds.0, K_AXVALUE_TYPE_CGRECT)
}

/// Copy an attribute with the transient-error retry — see
/// `FOCUS_RETRY_ATTEMPTS`.
fn copy_attr_retry(element: CFTypeRef, name: &'static str) -> Option<OwnedCF> {
    for attempt in 0..FOCUS_RETRY_ATTEMPTS {
        let attr = CFString::from_static_string(name);
        let mut value: CFTypeRef = std::ptr::null();
        // Safety: as in `copy_attr`.
        let err = unsafe {
            AXUIElementCopyAttributeValue(element, attr.as_concrete_TypeRef(), &mut value)
        };
        if err == 0 && !value.is_null() {
            return Some(OwnedCF(value));
        }
        // kAXErrorCannotComplete / kAXErrorNoValue: transient — the
        // app's own focus handling is mid-flight. Anything else will
        // fail the same way again; don't burn the budget on it.
        let transient = err == -25204 || err == -25212;
        if !transient || attempt + 1 == FOCUS_RETRY_ATTEMPTS {
            return None;
        }
        std::thread::sleep(FOCUS_RETRY_DELAY);
    }
    None
}

fn element_frame(element: CFTypeRef) -> Option<CGRect> {
    let origin = copy_attr(element, "AXPosition")
        .and_then(|v| ax_value::<CGPoint>(v.0, K_AXVALUE_TYPE_CGPOINT))?;
    let size = copy_attr(element, "AXSize")
        .and_then(|v| ax_value::<CGSize>(v.0, K_AXVALUE_TYPE_CGSIZE))?;
    Some(CGRect::new(&origin, &size))
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

#[cfg(test)]
mod tests;
