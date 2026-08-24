//! Shared XKB-code ↔ BCP-47 translation table for every Linux
//! backend. All Linux ecosystems (xkb, GNOME, KDE, IBus, Fcitx,
//! Hyprland) ultimately speak XKB short codes; we present BCP-47 to
//! the rest of poltertype.
//!
//! Keep extending this table as new layouts ship — adding an entry
//! here makes every Linux backend understand the new layout for free.

#![allow(dead_code)] // each backend uses a subset of these.

use std::path::PathBuf;

/// Pure-PATH lookup for a binary. We avoid the "run `foo --version`
/// and check exit status" trick because not every CLI we care about
/// has a working `--version` flag — Hyprland's `hyprctl` for example
/// prints its usage and exits 1 when given an unrecognised flag, so
/// the trick produces a false negative on a perfectly usable binary.
pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub fn cmd_exists(name: &str) -> bool {
    which(name).is_some()
}

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
        "bg" => "bg-BG",
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
        "bg-BG" => "bg",
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

/// Every variable a session announces its input method in. `XMODIFIERS`
/// is the X11-era one every toolkit still reads; the other two are how
/// GTK and Qt are told directly.
const IM_VARS: [&str; 3] = ["XMODIFIERS", "GTK_IM_MODULE", "QT_IM_MODULE"];

/// Is `name` the input method **this session actually uses**?
///
/// Running is not the same as chosen. Ubuntu installs fcitx5 with
/// language support and starts it at login, so `fcitx5-remote -t 1`
/// exits 0 on a desktop where fcitx owns nothing — and the backend then
/// reports whatever single keyboard engine it happens to hold, which is
/// enough to win the probe ahead of the X11 backend that would have
/// worked. Measured across the desktop matrix, 2026-08-24: eleven
/// sessions with every one of these variables empty and fcitx5 running.
///
/// A session that has genuinely adopted one says so: `XMODIFIERS=@im=fcitx`.
pub(crate) fn session_uses_input_method(name: &str) -> bool {
    IM_VARS
        .iter()
        .filter_map(|var| std::env::var(var).ok())
        .any(|value| value_names_im(&value, name))
}

/// `@im=fcitx`, or a bare `fcitx` in `GTK_IM_MODULE`. Matched whole so
/// `fcitx` and `ibus` can never claim each other, and the `5` suffix is
/// accepted because both spellings are in the wild.
fn value_names_im(value: &str, name: &str) -> bool {
    let token = value.rsplit("@im=").next().unwrap_or_default().trim();
    let matches = |candidate: &str| {
        candidate.eq_ignore_ascii_case(name) || candidate.eq_ignore_ascii_case(&format!("{name}5"))
    };
    matches(token) || matches(value.trim())
}

#[cfg(test)]
mod im_tests {
    use super::*;

    #[test]
    fn an_input_method_is_named_by_the_session_or_not_at_all() {
        assert!(value_names_im("@im=fcitx", "fcitx"));
        assert!(value_names_im("@im=fcitx5", "fcitx"));
        assert!(value_names_im("fcitx", "fcitx"));
        assert!(value_names_im("@im=ibus", "ibus"));
        assert!(value_names_im("ibus", "ibus"));

        assert!(!value_names_im("@im=ibus", "fcitx"));
        assert!(!value_names_im("@im=fcitx", "ibus"));
        // The measured case: fcitx5 running, nothing pointing at it.
        assert!(!value_names_im("", "fcitx"));
        assert!(!value_names_im("@im=none", "fcitx"));
        assert!(!value_names_im("gtk-im-context-simple", "ibus"));
    }
}
