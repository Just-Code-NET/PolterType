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

// ─── Switcher impl ───────────────────────────────────────────────────

pub struct MacosLayoutSwitcher;

impl MacosLayoutSwitcher {
    pub fn new() -> Self {
        Self
    }
}

impl LayoutSwitcher for MacosLayoutSwitcher {
    fn current(&self) -> Result<LayoutId, LayoutError> {
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

    fn list_active(&self) -> Result<Vec<LayoutId>, LayoutError> {
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

    fn switch_to(&self, id: &LayoutId) -> Result<(), LayoutError> {
        // Resolve LayoutId → TIS source by walking the active list and
        // matching IDs.
        let target_str = bcp47_to_tis_id(id.as_str()).unwrap_or_else(|| id.as_str().to_owned());

        // Safety: same lifetime story as `list_active`.
        unsafe {
            let arr_ref = TISCreateInputSourceList(std::ptr::null(), false);
            if arr_ref.is_null() {
                return Err(LayoutError::Os("TISCreateInputSourceList null".into()));
            }
            let arr: CFArray<CFTypeRef> = CFArray::wrap_under_create_rule(arr_ref);

            for src in arr.iter() {
                let cf = TISGetInputSourceProperty(*src, kTISPropertyInputSourceID);
                if cf.is_null() {
                    continue;
                }
                let s = CFString::wrap_under_get_rule(cf as CFStringRef).to_string();
                if s == target_str {
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

    fn backend_name(&self) -> &'static str {
        "macos-tis"
    }
}

unsafe fn source_id_to_layout_id(src: TISInputSourceRef) -> Option<LayoutId> {
    // Rust 2024: even inside an `unsafe fn`, calls to unsafe items
    // need an explicit unsafe block.
    let cf = unsafe { TISGetInputSourceProperty(src, kTISPropertyInputSourceID) };
    if cf.is_null() {
        return None;
    }
    let s = unsafe { CFString::wrap_under_get_rule(cf as CFStringRef) }.to_string();
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
            "com.apple.keylayout.USInternational-PC" => "en-US",
            "com.apple.keylayout.British" => "en-GB",
            "com.apple.keylayout.Ukrainian" => "uk-UA",
            "com.apple.keylayout.Ukrainian-PC" => "uk-UA",
            "com.apple.keylayout.Russian" => "ru-RU",
            "com.apple.keylayout.Russian-Phonetic" => "ru-RU",
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
