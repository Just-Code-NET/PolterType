//! macOS layout switcher via the Carbon TIS (Text Input Services) API.
//!
//! `TISCreateInputSourceList` enumerates installed keyboard layouts;
//! `TISSelectInputSource` makes one current.
//! `TISGetInputSourceProperty(s, kTISPropertyInputSourceID)` returns a
//! reverse-DNS string (`"com.apple.keylayout.US"`,
//! `"com.apple.keylayout.Ukrainian"`, …) which we map to BCP-47 with
//! a small table.
//!
//! > **Status:** written from Apple's documented behaviour and
//! > tested only via `cargo check` on macOS CI.

#![allow(unused_imports, dead_code)] // macOS-only; see DECISIONS for status.

use core_foundation::array::{CFArray, CFArrayRef};
use core_foundation::base::{CFRelease, CFTypeRef, OSStatus, TCFType};
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::string::{CFString, CFStringRef};

use crate::{LayoutError, LayoutId, LayoutSwitcher};

// ─── Carbon FFI ──────────────────────────────────────────────────────

#[allow(non_camel_case_types)]
type TISInputSourceRef = CFTypeRef;

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn TISCreateInputSourceList(
        properties: CFDictionaryRef,
        includeAllInstalled: bool,
    ) -> CFArrayRef;
    fn TISCopyCurrentKeyboardInputSource() -> TISInputSourceRef;
    fn TISSelectInputSource(inputSource: TISInputSourceRef) -> OSStatus;
    fn TISGetInputSourceProperty(
        inputSource: TISInputSourceRef,
        propertyKey: CFStringRef,
    ) -> CFTypeRef;
    static kTISPropertyInputSourceID: CFStringRef;
    static kTISPropertyInputSourceCategory: CFStringRef;
    static kTISCategoryKeyboardInputSource: CFStringRef;
    static kTISPropertyInputSourceIsSelectCapable: CFStringRef;
}

// ─── Main-thread dispatch ────────────────────────────────────────────
//
// Since macOS 14/15 the HIToolbox Text-Services layer asserts the
// main dispatch queue (`dispatch_assert_queue` inside
// `TSMGetInputSourceProperty` → `isValidateInputSourceRef` →
// `islGetInputSourceListWithAdditions`). Calling ANY TIS function
// from a worker thread — our layout poller polls every 250 ms, the
// engine switches on its own thread — kills the process with SIGILL
// (EXC_BAD_INSTRUCTION). Route every TIS call through the main
// dispatch queue; run inline when the caller already is the main
// thread, or `dispatch_sync` would deadlock.

use std::ffi::c_void;

type DispatchQueueT = *mut c_void;

unsafe extern "C" {
    /// The main dispatch queue. Modern libdispatch headers alias
    /// `dispatch_get_main_queue()` to this global (`DISPATCH_GLOBAL_OBJECT`),
    /// and the function symbol itself is no longer exported by
    /// libSystem — so we link the object directly, like the `dispatch`
    /// crate does.
    static _dispatch_main_q: c_void;
    fn dispatch_sync_f(
        queue: DispatchQueueT,
        context: *mut c_void,
        work: extern "C" fn(*mut c_void),
    );
    fn pthread_main_np() -> i32;
}

/// Run `f` on the main dispatch queue and return its result.
///
/// Ordering note: callers on worker threads (the layout poller, the
/// engine) can arrive *before* the tao event loop starts draining the
/// main queue. They simply block in `dispatch_sync_f` until the run
/// loop starts — never a deadlock, because the main thread itself
/// takes the inline path and never waits on a worker.
fn run_on_main<T, F>(f: F) -> T
where
    T: Send,
    F: FnOnce() -> T + Send,
{
    if unsafe { pthread_main_np() } == 1 {
        return f();
    }
    struct Ctx<T, F> {
        f: Option<F>,
        out: Option<T>,
    }
    extern "C" fn trampoline<T, F>(p: *mut c_void)
    where
        F: FnOnce() -> T,
    {
        // Safety: `dispatch_sync_f` guarantees `p` points to the live
        // `Ctx` on the caller's stack for the duration of this call.
        let ctx = unsafe { &mut *(p as *mut Ctx<T, F>) };
        if let Some(f) = ctx.f.take() {
            ctx.out = Some(f());
        }
    }
    let mut ctx = Ctx {
        f: Some(f),
        out: None,
    };
    unsafe {
        dispatch_sync_f(
            &raw const _dispatch_main_q as DispatchQueueT,
            &mut ctx as *mut Ctx<T, F> as *mut c_void,
            trampoline::<T, F>,
        );
    }
    match ctx.out {
        Some(v) => v,
        // dispatch_sync_f returns only after the closure ran, so the
        // slot is always filled; this branch is here because the
        // compiler can't see through the FFI boundary.
        None => unreachable!("dispatch_sync_f returned without running the closure"),
    }
}

// ─── Switcher impl ───────────────────────────────────────────────────

pub struct MacosLayoutSwitcher;

impl MacosLayoutSwitcher {
    pub fn new() -> Self {
        Self
    }
}

impl LayoutSwitcher for MacosLayoutSwitcher {
    fn current(&self) -> Result<LayoutId, LayoutError> {
        run_on_main(current_impl)
    }

    fn list_active(&self) -> Result<Vec<LayoutId>, LayoutError> {
        run_on_main(list_active_impl)
    }

    fn switch_to(&self, id: &LayoutId) -> Result<(), LayoutError> {
        let id = id.clone();
        run_on_main(move || switch_to_impl(&id))
    }

    fn backend_name(&self) -> &'static str {
        "macos-tis"
    }
}

fn current_impl() -> Result<LayoutId, LayoutError> {
    // Safety: TISCopyCurrentKeyboardInputSource always returns a
    // retained source we must release after reading the ID.
    unsafe {
        let src = TISCopyCurrentKeyboardInputSource();
        if src.is_null() {
            return Err(LayoutError::Os(
                "TISCopyCurrentKeyboardInputSource returned null".into(),
            ));
        }
        let id = source_id_to_layout_id(src);
        CFRelease(src);
        id.ok_or_else(|| LayoutError::Os("could not read TIS InputSourceID".into()))
    }
}

fn list_active_impl() -> Result<Vec<LayoutId>, LayoutError> {
    // Filter for keyboard sources that the user can select.
    let filter = unsafe {
        let category_key = CFString::wrap_under_get_rule(kTISPropertyInputSourceCategory);
        let category_val = CFString::wrap_under_get_rule(kTISCategoryKeyboardInputSource);
        let select_key = CFString::wrap_under_get_rule(kTISPropertyInputSourceIsSelectCapable);
        // Build a dictionary `{ category = keyboard, select = true }`.
        let true_val = core_foundation::boolean::CFBoolean::true_value();
        CFDictionary::from_CFType_pairs(&[
            (category_key.as_CFType(), category_val.as_CFType()),
            (select_key.as_CFType(), true_val.as_CFType()),
        ])
    };

    // Safety: TISCreateInputSourceList retains the array; we wrap
    // it for proper Drop. Each source inside is a CFTypeRef we
    // read via the property accessor (no extra retain).
    unsafe {
        let arr_ref = TISCreateInputSourceList(filter.as_concrete_TypeRef(), false);
        if arr_ref.is_null() {
            return Ok(Vec::new());
        }
        let arr: CFArray<CFTypeRef> = CFArray::wrap_under_create_rule(arr_ref);
        let mut out = Vec::with_capacity(arr.len() as usize);
        for src in arr.iter() {
            if let Some(id) = source_id_to_layout_id(*src) {
                out.push(id);
            }
        }
        Ok(out)
    }
}

fn switch_to_impl(id: &LayoutId) -> Result<(), LayoutError> {
    // Resolve LayoutId → TIS source by walking the active list and
    // matching IDs. A source matches when its raw TIS ID equals the
    // reverse-mapped target *or* its forward-mapped BCP-47 equals the
    // requested id — so `ru-RU` finds `com.apple.keylayout.RussianWin`
    // on machines that only have the PC variant installed.
    let target_str = bcp47_to_tis_id(id.as_str()).unwrap_or_else(|| id.as_str().to_owned());

    // Safety: same lifetime story as `list_active_impl`.
    unsafe {
        let arr_ref = TISCreateInputSourceList(std::ptr::null(), false);
        if arr_ref.is_null() {
            return Err(LayoutError::Os("TISCreateInputSourceList null".into()));
        }
        let arr: CFArray<CFTypeRef> = CFArray::wrap_under_create_rule(arr_ref);

        for src in arr.iter() {
            let Some(raw) = source_id_string(*src) else {
                continue;
            };
            if raw == target_str || tis_id_to_bcp47(&raw).as_deref() == Some(id.as_str()) {
                let st = TISSelectInputSource(*src);
                if st != 0 {
                    return Err(LayoutError::Os(format!(
                        "TISSelectInputSource returned OSStatus {st}"
                    )));
                }
                return Ok(());
            }
        }
    }

    Err(LayoutError::NotActive(id.clone()))
}

unsafe fn source_id_string(src: TISInputSourceRef) -> Option<String> {
    let cf = unsafe { TISGetInputSourceProperty(src, kTISPropertyInputSourceID) };
    if cf.is_null() {
        return None;
    }
    Some(unsafe { CFString::wrap_under_get_rule(cf as CFStringRef) }.to_string())
}

unsafe fn source_id_to_layout_id(src: TISInputSourceRef) -> Option<LayoutId> {
    let s = unsafe { source_id_string(src) }?;
    Some(LayoutId::new(tis_id_to_bcp47(&s).unwrap_or(s)))
}

// ─── ID translation tables ───────────────────────────────────────────

/// `"com.apple.keylayout.US"` → `"en-US"`. Tiny built-in table for the
/// layouts most likely to be enabled. Anything unmapped falls through
/// as the raw TIS ID, which is still a stable opaque LayoutId.
fn tis_id_to_bcp47(id: &str) -> Option<String> {
    Some(
        match id {
            "com.apple.keylayout.US" => "en-US",
            "com.apple.keylayout.ABC" => "en-US",
            "com.apple.keylayout.USInternational-PC" => "en-US",
            "com.apple.keylayout.British" => "en-GB",
            "com.apple.keylayout.Ukrainian" => "uk-UA",
            "com.apple.keylayout.Ukrainian-PC" => "uk-UA",
            "com.apple.keylayout.UkrainianWin" => "uk-UA",
            "com.apple.keylayout.Russian" => "ru-RU",
            "com.apple.keylayout.Russian-Phonetic" => "ru-RU",
            "com.apple.keylayout.RussianWin" => "ru-RU",
            "com.apple.keylayout.German" => "de-DE",
            "com.apple.keylayout.French" => "fr-FR",
            "com.apple.keylayout.Spanish" => "es-ES",
            "com.apple.keylayout.Polish" => "pl-PL",
            "com.apple.keylayout.Greek" => "el-GR",
            _ => return None,
        }
        .to_owned(),
    )
}

/// Inverse of [`tis_id_to_bcp47`] — best-effort reverse lookup so
/// `switch_to(LayoutId("uk-UA"))` finds the right TIS source. Falls
/// back to the input string if no entry exists.
fn bcp47_to_tis_id(id: &str) -> Option<String> {
    Some(
        match id {
            "en-US" => "com.apple.keylayout.US",
            "en-GB" => "com.apple.keylayout.British",
            "uk-UA" => "com.apple.keylayout.Ukrainian-PC",
            "ru-RU" => "com.apple.keylayout.Russian",
            "de-DE" => "com.apple.keylayout.German",
            "fr-FR" => "com.apple.keylayout.French",
            "es-ES" => "com.apple.keylayout.Spanish",
            "pl-PL" => "com.apple.keylayout.Polish",
            "el-GR" => "com.apple.keylayout.Greek",
            _ => return None,
        }
        .to_owned(),
    )
}
