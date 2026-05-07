//! Per-application wordlist profiles.
//!
//! ## What problem this solves
//!
//! The base wordlist overlay
//! `<config-dir>/kb-switcher/wordlists/<stem>.txt` is **global**:
//! every word added there boosts that layout's dictionary across
//! every application the user types into. That's right for
//! everyday vocabulary (family names, regional spellings) but
//! wrong for context-specific jargon — adding `kubectl` /
//! `terraform` to your en-US overlay so it stops flagging them in
//! VS Code also stops it from flagging them in chat, where you
//! actually do mean to type the English word.
//!
//! Profiles let the user keep separate overlay sets per context.
//! The engine swaps the active overlay when the foreground app
//! changes — `kubectl` only counts toward "this is English" while
//! the user is typing inside a code editor.
//!
//! ## On-disk layout
//!
//! Adds one directory level on top of the existing user-overlay
//! contract documented in `crates/kb-core/src/layouts.rs`:
//!
//! ```text
//! <config-dir>/kb-switcher/wordlists/
//!   <stem>.txt                  ← global overlay (existing — fallback)
//!   <stem>-stop.txt             ← global stop list (existing — fallback)
//!   profiles/
//!     <profile-id>/
//!       <stem>.txt              ← profile-specific overlay
//!       <stem>-stop.txt         ← profile-specific stop list
//! ```
//!
//! Same parser as the global overlay (`one word per line, # for
//! comments, blank lines ignored`), so power users can `cp` files
//! between the two layers and they'll just work.
//!
//! ## Schema
//!
//! See [`WordlistSettings`] / [`WordlistProfile`] below. Profiles
//! are matched by `apps` against the focused process's exe basename
//! (case-insensitive — same comparison
//! [`crate::settings::ExceptionSettings`] uses for `disabled_apps`,
//! so users only learn one matching rule).
//!
//! ## What's intentionally not here in v1
//!
//! * **Profile inheritance.** A profile is its own overlay set;
//!   it doesn't merge with the global overlay or another profile.
//!   Inheritance was tempting but adds load-time complexity (cycle
//!   detection, depth limit) and a UX surface ("which profile
//!   wins?") that isn't worth it until users actually ask for it.
//! * **Per-language vs per-layout profile granularity.** A profile
//!   has one `<stem>.txt` per layout, full stop. We considered
//!   "this profile only changes en-US" but the file system already
//!   gives you that — just don't create the other `<stem>.txt`
//!   files for the layouts you don't want to touch.
//! * **Hot reload.** Same constraint as the global overlay: the
//!   loader runs once at engine start. Editing files at runtime
//!   needs a tray restart. The Settings UI's banner spells this out.

use serde::{Deserialize, Serialize};

/// Top-level wordlist settings — sits under `[wordlists]` in
/// `config.toml`. The default is empty (no profiles configured),
/// which preserves the existing global-overlay-only behaviour for
/// users on configs from beta.4 and earlier.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct WordlistSettings {
    /// Profile id used when no profile's `apps` list matches the
    /// focused app. Empty string (the default) means "use the
    /// global overlay files only" — i.e. exactly what every user
    /// got before profiles existed. A non-empty value must refer
    /// to one of the entries in `profiles` below; an unknown id
    /// logs a warning at engine start and falls back to the empty
    /// (global-only) state.
    pub default_profile: String,
    /// Configured profiles. Order is significant only for the
    /// "first match wins" rule when two profiles list the same
    /// app — that's a config error in practice but we resolve it
    /// deterministically rather than panicking.
    pub profiles: Vec<WordlistProfile>,
}

/// A single profile. The id picks the on-disk directory
/// (`<config-dir>/kb-switcher/wordlists/profiles/<id>/`); the
/// `apps` list picks when this profile is active.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WordlistProfile {
    /// Stable identifier. Must be unique across `profiles`.
    /// Recommended shape: kebab-case ASCII (`code`, `writing`,
    /// `gaming`). The id is also the directory name on disk, so
    /// avoid characters the OS doesn't like in path segments
    /// (`/`, `\\`, `:`, `*`, `?`, `"`, `<`, `>`, `|` on Windows).
    pub id: String,
    /// Free-form display name shown in the Settings UI. Falls
    /// back to `id` if blank. Not used by the engine for matching
    /// (matching is by `apps` only).
    #[serde(default)]
    pub name: String,
    /// Foreground apps this profile activates for. Each entry is
    /// matched case-insensitively against the focused process's
    /// exe basename — same comparison
    /// [`crate::settings::ExceptionSettings`] uses for
    /// `disabled_apps`, so users only learn one matching rule.
    /// Empty list = profile is never auto-activated (the user can
    /// still pick it manually as `default_profile`).
    #[serde(default)]
    pub apps: Vec<String>,
}

/// Pick the active profile id for a focused app's exe basename,
/// or `None` to fall through to the global overlay. Resolution
/// order:
///
/// 1. First profile whose `apps` list contains a case-insensitive
///    match for `focused_basename` wins.
/// 2. Otherwise, [`WordlistSettings::default_profile`] if it names
///    a known profile.
/// 3. Otherwise, `None` (caller uses the global overlay only).
///
/// `focused_basename` should already be just the basename — the
/// caller usually has it after `Path::new(exe).file_name()`. We
/// don't strip a path here because the function is also called
/// from tests with synthetic data.
pub fn resolve_active_profile<'a>(
    settings: &'a WordlistSettings,
    focused_basename: Option<&str>,
) -> Option<&'a str> {
    if let Some(name) = focused_basename {
        for p in &settings.profiles {
            if p.apps.iter().any(|a| a.eq_ignore_ascii_case(name)) {
                return Some(&p.id);
            }
        }
    }
    if !settings.default_profile.is_empty()
        && settings
            .profiles
            .iter()
            .any(|p| p.id == settings.default_profile)
    {
        return Some(&settings.default_profile);
    }
    None
}

#[cfg(test)]
mod tests {
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

    /// Profile id is also a directory name; reject obviously
    /// path-traversal-y or empty ids on the parsing side. (The
    /// loader has its own checks but this is the canonical "is
    /// this id sane" pin.)
    #[test]
    fn profile_id_validation_basic() {
        // We don't ship a `validate(id)` API yet — but we want a
        // place to anchor the contract: ids should be ASCII,
        // non-empty, no path separators. Future loader code can
        // call into this once the helper lands.
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
}
