//! Reading a TIS source's raw `InputSourceID` and translating it to
//! and from BCP-47.
//!
//! The two translation tables are deliberately not maintained as
//! inverses of each other — see [`bcp47_to_tis_id`] for why.

use core_foundation::base::TCFType;
use core_foundation::string::{CFString, CFStringRef};

use crate::LayoutId;

use super::{TISGetInputSourceProperty, TISInputSourceRef, kTISPropertyInputSourceID};

pub(super) unsafe fn source_id_string(src: TISInputSourceRef) -> Option<String> {
    let cf = unsafe { TISGetInputSourceProperty(src, kTISPropertyInputSourceID) };
    if cf.is_null() {
        return None;
    }
    Some(unsafe { CFString::wrap_under_get_rule(cf as CFStringRef) }.to_string())
}

pub(super) unsafe fn source_id_to_layout_id(src: TISInputSourceRef) -> Option<LayoutId> {
    let s = unsafe { source_id_string(src) }?;
    Some(LayoutId::new(tis_id_to_bcp47(&s).unwrap_or(s)))
}

/// `"com.apple.keylayout.US"` → `"en-US"`. Tiny built-in table for the
/// layouts most likely to be enabled. Anything unmapped falls through
/// as the raw TIS ID, which is still a stable opaque LayoutId.
pub(super) fn tis_id_to_bcp47(id: &str) -> Option<String> {
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
            "com.apple.keylayout.PolishPro" => "pl-PL",
            "com.apple.keylayout.Greek" => "el-GR",
            "com.apple.keylayout.GreekPolytonic" => "el-GR",
            "com.apple.keylayout.Czech" => "cs-CZ",
            "com.apple.keylayout.Czech-QWERTY" => "cs-CZ",
            "com.apple.keylayout.Hebrew" => "he-IL",
            "com.apple.keylayout.Hebrew-PC" => "he-IL",
            "com.apple.keylayout.Hebrew-QWERTY" => "he-IL",
            "com.apple.keylayout.Turkish" => "tr-TR",
            "com.apple.keylayout.Turkish-Standard" => "tr-TR",
            "com.apple.keylayout.Turkish-QWERTY-PC" => "tr-TR",
            "com.apple.keylayout.Bulgarian" => "bg-BG",
            "com.apple.keylayout.Bulgarian-Phonetic" => "bg-BG",
            "com.apple.keylayout.Italian" => "it-IT",
            "com.apple.keylayout.Italian-Pro" => "it-IT",
            "com.apple.keylayout.Portuguese" => "pt-PT",
            "com.apple.keylayout.Brazilian" => "pt-BR",
            _ => return None,
        }
        .to_owned(),
    )
}

/// Inverse of [`tis_id_to_bcp47`], so `switch_to(LayoutId("uk-UA"))`
/// finds the right TIS source. Falls back to the input string.
///
/// Deliberately narrower than the forward table: this names the source
/// we *ask macOS to select*, so only the base id each language is
/// certain to have gets an entry. A wrong guess forward costs nothing —
/// the id stays a stable opaque `LayoutId` — whereas a wrong guess here
/// silently targets a source the user does not have.
pub(super) fn bcp47_to_tis_id(id: &str) -> Option<String> {
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
            "cs-CZ" => "com.apple.keylayout.Czech",
            "he-IL" => "com.apple.keylayout.Hebrew",
            "tr-TR" => "com.apple.keylayout.Turkish",
            "bg-BG" => "com.apple.keylayout.Bulgarian",
            "it-IT" => "com.apple.keylayout.Italian",
            "pt-PT" => "com.apple.keylayout.Portuguese",
            "pt-BR" => "com.apple.keylayout.Brazilian",
            _ => return None,
        }
        .to_owned(),
    )
}
