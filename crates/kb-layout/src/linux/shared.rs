//! Shared XKB-code ↔ BCP-47 translation table for every Linux
//! backend. All Linux ecosystems (xkb, GNOME, KDE, IBus, Fcitx,
//! Hyprland) ultimately speak XKB short codes; we present BCP-47 to
//! the rest of kb-switcher.
//!
//! Keep extending this table as new layouts ship — adding an entry
//! here makes every Linux backend understand the new layout for free.

#![allow(dead_code)] // each backend uses a subset of these.

pub fn xkb_to_bcp47(code: &str) -> Option<&'static str> {
    Some(match code {
        "us" => "en-US",
        "gb" => "en-GB",
        "ua" => "uk-UA",
        "ru" => "ru-RU",
        "de" => "de-DE",
        "fr" => "fr-FR",
        "es" => "es-ES",
        "pl" => "pl-PL",
        "gr" => "el-GR",
        "it" => "it-IT",
        "pt" => "pt-PT",
        "br" => "pt-BR",
        "tr" => "tr-TR",
        "cz" => "cs-CZ",
        "sk" => "sk-SK",
        "ro" => "ro-RO",
        "hu" => "hu-HU",
        "nl" => "nl-NL",
        "be" => "nl-BE",
        "se" => "sv-SE",
        "no" => "no-NO",
        "dk" => "da-DK",
        "fi" => "fi-FI",
        "kz" => "kk-Cyrl-KZ",
        "by" => "be-BY",
        "am" => "hy-AM",
        "ge" => "ka-GE",
        "il" => "he-IL",
        "ara" => "ar",
        "jp" => "ja-JP",
        "kr" => "ko-KR",
        _ => return None,
    })
}

pub fn bcp47_to_xkb(bcp: &str) -> Option<&'static str> {
    Some(match bcp {
        "en-US" => "us",
        "en-GB" => "gb",
        "uk-UA" => "ua",
        "ru-RU" => "ru",
        "de-DE" => "de",
        "fr-FR" => "fr",
        "es-ES" => "es",
        "pl-PL" => "pl",
        "el-GR" => "gr",
        "it-IT" => "it",
        "pt-PT" => "pt",
        "pt-BR" => "br",
        "tr-TR" => "tr",
        "cs-CZ" => "cz",
        "sk-SK" => "sk",
        "ro-RO" => "ro",
        "hu-HU" => "hu",
        "nl-NL" => "nl",
        "nl-BE" => "be",
        "sv-SE" => "se",
        "no-NO" => "no",
        "da-DK" => "dk",
        "fi-FI" => "fi",
        "kk-Cyrl-KZ" => "kz",
        "be-BY" => "by",
        "hy-AM" => "am",
        "ka-GE" => "ge",
        "he-IL" => "il",
        "ja-JP" => "jp",
        "ko-KR" => "kr",
        _ => return None,
    })
}
