use serde::Deserialize;

use super::*;

#[test]
fn defaults_are_empty() {
    let s = WordlistSettings::default();
    assert!(s.default_profile.is_empty());
    assert!(s.profiles.is_empty());
}

/// A complete config block users might write — array-of-tables
/// `[[wordlists.profiles]]` parses end-to-end through serde.
#[test]
fn parses_complete_wordlists_block() {
    let raw = r#"
[wordlists]
default_profile = "writing"

[[wordlists.profiles]]
id     = "code"
name   = "Programming"
apps   = ["Code.exe", "Cursor.exe", "idea64.exe"]

[[wordlists.profiles]]
id     = "writing"
name   = "Long-form prose"
apps   = ["WINWORD.EXE", "obsidian.exe"]
"#;
    // We mirror the wrapper used in `Settings.wordlists`.
    #[derive(Deserialize)]
    struct Wrap {
        #[serde(default)]
        wordlists: WordlistSettings,
    }
    let w: Wrap = toml::from_str(raw).expect("parse");
    assert_eq!(w.wordlists.default_profile, "writing");
    assert_eq!(w.wordlists.profiles.len(), 2);

    let code = &w.wordlists.profiles[0];
    assert_eq!(code.id, "code");
    assert_eq!(code.name, "Programming");
    assert!(code.apps.iter().any(|a| a == "Code.exe"));

    let writing = &w.wordlists.profiles[1];
    assert_eq!(writing.id, "writing");
    assert_eq!(writing.apps.len(), 2);
}

/// Resolution: focused app matches → its profile wins. Tests
/// the case-insensitive comparison, the "first match wins"
/// rule, and the basename contract (caller supplies basename,
/// we don't strip path).
#[test]
fn resolve_picks_first_app_match() {
    let s = WordlistSettings {
        default_profile: String::new(),
        profiles: vec![
            WordlistProfile {
                id: "code".into(),
                name: String::new(),
                apps: vec!["Code.exe".into(), "idea64.exe".into()],
            },
            WordlistProfile {
                id: "writing".into(),
                name: String::new(),
                apps: vec!["WINWORD.EXE".into()],
            },
        ],
    };

    assert_eq!(resolve_active_profile(&s, Some("Code.exe")), Some("code"));
    // Case-insensitive match.
    assert_eq!(resolve_active_profile(&s, Some("CODE.EXE")), Some("code"));
    // Different profile.
    assert_eq!(
        resolve_active_profile(&s, Some("winword.exe")),
        Some("writing")
    );
    // No app provided → no profile active.
    assert_eq!(resolve_active_profile(&s, None), None);
    // App not in any profile → fall through.
    assert_eq!(resolve_active_profile(&s, Some("chrome.exe")), None);
}

/// `default_profile` is the per-config fallback when no app
/// matches. An unknown id (typo, deleted profile) must be
/// treated as "no profile" rather than crashing the engine.
#[test]
fn resolve_falls_back_to_default_profile() {
    let s = WordlistSettings {
        default_profile: "code".into(),
        profiles: vec![WordlistProfile {
            id: "code".into(),
            name: String::new(),
            apps: vec!["Code.exe".into()],
        }],
    };

    // Focused app doesn't match; fall through to default.
    assert_eq!(resolve_active_profile(&s, Some("chrome.exe")), Some("code"));

    // Now break `default_profile` — it points at a profile
    // that doesn't exist. We must NOT pick it.
    let mut bad = s.clone();
    bad.default_profile = "ghost".into();
    assert_eq!(resolve_active_profile(&bad, Some("chrome.exe")), None);
}

/// Profile id is also a directory name; the loader has its own checks,
/// but this is the canonical "is this id sane" pin.
#[test]
fn profile_id_validation_basic() {
    // Local, because there is deliberately no shipped `validate(id)`
    // API: this pins the contract, not an implementation.
    fn looks_safe(id: &str) -> bool {
        !id.is_empty()
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    }
    for ok in ["code", "writing", "gaming-2", "long_form"] {
        assert!(looks_safe(ok));
    }
    for bad in ["", "../etc", "with space", "slash/dir", "back\\slash"] {
        assert!(!looks_safe(bad), "{bad} should be rejected");
    }
}
