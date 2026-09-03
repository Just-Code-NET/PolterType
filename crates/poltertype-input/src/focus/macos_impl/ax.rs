//! Raw AX/CF attribute queries: copying an attribute value, decoding
//! an `AXValue`, and the transient-error retry every focus query
//! needs when it races the target app's own focus handling.

use core_foundation::base::{CFGetTypeID, CFTypeRef, TCFType};
use core_foundation::string::{CFString, CFStringGetTypeID};
use core_graphics::geometry::{CGPoint, CGRect, CGSize};

use super::consts::{
    FOCUS_RETRY_ATTEMPTS, FOCUS_RETRY_DELAY, K_AXVALUE_TYPE_CGPOINT, K_AXVALUE_TYPE_CGRECT,
    K_AXVALUE_TYPE_CGSIZE,
};
use super::types::OwnedCF;
use super::{
    AXUIElementCopyAttributeValue, AXUIElementCopyParameterizedAttributeValue, AXValueGetValue,
};

/// Copy an attribute, or `None` on any AX error (no permission, no
/// focused element, app without a11y — all mean "degrade gracefully").
pub(super) fn copy_attr(element: CFTypeRef, name: &'static str) -> Option<OwnedCF> {
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

pub(super) fn copy_string_attr(element: CFTypeRef, name: &'static str) -> Option<String> {
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

pub(super) fn ax_value<T: Copy>(value: CFTypeRef, value_type: u32) -> Option<T> {
    let mut out: T = unsafe { std::mem::zeroed() };
    // Safety: `out` is the exact payload type `value_type` promises to
    // write, and `value` is a live AXValue we own.
    let ok = unsafe { AXValueGetValue(value, value_type, std::ptr::from_mut(&mut out).cast()) };
    if ok { Some(out) } else { None }
}

/// A parameterized attribute whose answer is an AXValue-wrapped
/// CGRect (`kAXBoundsForRange`, `AXBoundsForTextMarkerRange`).
pub(super) fn parameterized_rect(
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
pub(super) fn copy_attr_retry(element: CFTypeRef, name: &'static str) -> Option<OwnedCF> {
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

pub(super) fn element_frame(element: CFTypeRef) -> Option<CGRect> {
    let origin = copy_attr(element, "AXPosition")
        .and_then(|v| ax_value::<CGPoint>(v.0, K_AXVALUE_TYPE_CGPOINT))?;
    let size = copy_attr(element, "AXSize")
        .and_then(|v| ax_value::<CGSize>(v.0, K_AXVALUE_TYPE_CGSIZE))?;
    Some(CGRect::new(&origin, &size))
}
