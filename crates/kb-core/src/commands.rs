//! Smart commands — text-trigger expansions and shortcuts.
//!
//! Inspired by classic text expanders (TextExpander, Espanso,
//! AutoHotkey hotstrings): the user types a short token like
//! `anrl ` (acronym + space), the engine recognises it on the word
//! boundary, deletes the token + boundary, and runs an action —
//! typically expanding to a longer phrase.
//!
//! ## Why text triggers, not hotkeys
//!
//! `[hotkeys]` already gives users two global key combinations
//! (pause, switch-last). Adding more global hotkeys is a separate
//! UX choice — they collide with system-wide bindings, they're
//! invisible (a typed trigger is right there in your text), and
//! the OS imposes a hard limit on how many you can register. Text
//! triggers don't have any of those constraints — they live
//! entirely inside kb-switcher's word-boundary pipeline (the same
//! pipeline that already does layout-aware corrections), so users
//! can have hundreds of them with no performance / UX cost.
//!
//! ## Activation
//!
//! The engine consults the configured triggers on every word
//! boundary, BEFORE the structural-boundary / disabled-app /
//! identifier filters. Order is significant:
//!
//!   1. User types `anrl<space>`.
//!   2. Word boundary fires.
//!   3. Trigger lookup: `anrl` matches → dispatch action,
//!      backspace the typed token + boundary, re-emit any text
//!      the action wants to leave behind, return.
//!   4. (Otherwise) normal layout-correction pipeline runs.
//!
//! Putting trigger lookup BEFORE the identifier / app filters means
//! a snippet like `=>` works inside an IDE — those filters would
//! otherwise veto auto-switching, but text expansion is what the
//! user actively asked for, so the filters don't apply.
//!
//! ## v1 action surface
//!
//! Deliberately small. Each variant maps to one OS primitive we
//! already know how to do safely:
//!
//! * [`CommandAction::TypeText`] → `KeyEmitter::send_text`
//! * [`CommandAction::SwitchLayout`] → `LayoutSwitcher::switch_to`
//! * [`CommandAction::OpenPath`] → `opener::open`
//!
//! The most common use case is `TypeText` (snippet expansion); the
//! other two are power-user shortcuts that happen to fit the same
//! "type a magic word, something happens" model.
//!
//! What's intentionally **not** here in v1:
//!
//! * `RunShell { argv }` — full command execution. The blast radius
//!   (a malicious `[[commands]]` entry in a stolen config could
//!   mass-exfiltrate) makes this a separate security review.
//! * Multi-token triggers (`best regards` → `…`). The buffer is
//!   reset at every word boundary; matching across boundaries
//!   needs a sliding window we don't have today.
//! * Case-insensitive / case-preserving expansion. v1 matches
//!   exactly — users pick triggers that don't collide with prose.

use kb_types::LayoutId;
use serde::{Deserialize, Serialize};

/// Reserved built-in command ids — defined here so user-side schema
/// validation can warn when a `[[commands]]` entry tries to shadow
/// one. The engine's two built-in hotkey actions
/// (`pause-toggle` and `switch-last`) live in [`crate::settings::HotkeySettings`]
/// and are registered separately by the tray; a user-side
/// `[[commands]]` entry with the same id would still be a smart
/// command (text trigger), not a hotkey replacement — but reusing
/// the names is confusing, hence the reservation.
pub const BUILTIN_PAUSE_TOGGLE_ID: &str = "pause-toggle";
pub const BUILTIN_SWITCH_LAST_ID: &str = "switch-last";

/// A single user-defined smart command. Saved as a `[[commands]]`
/// entry in `config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserCommand {
    /// Stable identifier. Must be unique across the user's command
    /// list. Used in logs and as a stable handle for the UI; never
    /// shown to end users on its own (we render `name` instead).
    /// Must NOT collide with [`BUILTIN_PAUSE_TOGGLE_ID`] or
    /// [`BUILTIN_SWITCH_LAST_ID`].
    pub id: String,
    /// Display name shown in the Settings UI's Commands pane.
    /// Free-form; falls back to `id` if empty.
    #[serde(default)]
    pub name: String,
    /// The token the user types to fire this command. Matched
    /// exactly (case-sensitive) against the just-completed word
    /// at every boundary. Must not contain whitespace — the buffer
    /// resets at word boundaries, so a multi-token trigger could
    /// never match.
    ///
    /// Examples:
    ///
    /// * `anrl` — common acronym, expands to a phrase.
    /// * `:date:` — Espanso-style delimited trigger that won't
    ///   collide with normal words.
    /// * `;sig` — leading punctuation that's also rare in prose.
    ///
    /// Choose triggers that don't collide with words you actually
    /// type — `the` would expand on every English sentence.
    pub trigger: String,
    /// What to do when the trigger fires. Tagged-union TOML — the
    /// `type` key picks the variant.
    pub action: CommandAction,
    /// Optional list of foreground app basenames this command is
    /// active in. Empty = active everywhere. Match is case-
    /// insensitive against the focused process's exe basename, the
    /// same comparison [`crate::settings::ExceptionSettings`] uses.
    #[serde(default)]
    pub apps: Vec<String>,
}

/// Tagged enum of available actions. The TOML representation uses
/// `type = "<snake_case>"` plus the variant's payload fields, e.g.
///
/// ```toml
/// [[commands]]
/// id      = "anrl"
/// name    = "Anatomical reference list"
/// trigger = "anrl"
/// action  = { type = "type_text", text = "Anatomical Reference List" }
///
/// [[commands]]
/// id      = "to-english"
/// trigger = "((en))"
/// action  = { type = "switch_layout", layout = "en-US" }
///
/// [[commands]]
/// id      = "open-config"
/// trigger = ";cfg"
/// action  = { type = "open_path", path = "C:/Users/me/AppData/Roaming/kb-switcher/config.toml" }
/// ```
///
/// Adding a new variant is forward-compat: an old binary will fail
/// to parse the unknown `type` and the loader will keep the rest of
/// the config (one warning logged per skipped entry).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CommandAction {
    /// Type a literal string at the cursor. The engine first
    /// backspaces the trigger + the boundary character the user
    /// just typed, then emits this text, then re-emits the
    /// boundary character. So typing `anrl<space>` with text
    /// `Anatomical Reference List` produces
    /// `Anatomical Reference List<space>` — the space the user
    /// typed survives the expansion.
    TypeText { text: String },
    /// Switch the OS keyboard layout to the given id. Same
    /// pre-flight (`list_active`) as the corrector uses, so an
    /// unreachable layout is rejected loudly rather than silently.
    /// The trigger + boundary are deleted; nothing is re-emitted.
    SwitchLayout { layout: LayoutId },
    /// Open a path or URL via the user's default handler
    /// (`opener` crate). Files use the OS's MIME / extension
    /// mapping; URLs get the default browser. Trigger + boundary
    /// are deleted; nothing is re-emitted.
    OpenPath { path: String },
}

/// Look up the first command in `commands` whose `trigger` exactly
/// matches `typed_word` and whose `apps` filter (if set) accepts
/// `focused_basename`. Returns `None` if no command matches.
///
/// The lookup is linear because the typical user has ≤ 20
/// commands; a hash map would add complexity without measurable
/// benefit and would lose the deterministic "first match wins"
/// rule when two commands share a trigger (a config error in
/// practice — but we resolve it predictably rather than crashing).
///
/// Match is case-sensitive on the trigger by design. Most snippet
/// expanders end up wanting case-sensitive matching once users
/// have triggers like `Anrl` (capitalised expansion) vs `anrl`
/// (lowercase one).
pub fn find_matching_command<'a>(
    commands: &'a [UserCommand],
    typed_word: &str,
    focused_basename: Option<&str>,
) -> Option<&'a UserCommand> {
    commands.iter().find(|c| {
        c.trigger == typed_word
            && (c.apps.is_empty()
                || focused_basename
                    .is_some_and(|b| c.apps.iter().any(|a| a.eq_ignore_ascii_case(b))))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The TOML-tag form must round-trip every variant — that's the
    /// shape users will type into `config.toml`, so any rename or
    /// field reorder breaks every existing config.
    #[test]
    fn each_action_variant_round_trips_through_toml() {
        let cases = [
            (
                "type_text",
                CommandAction::TypeText {
                    text: "hello".into(),
                },
            ),
            (
                "switch_layout",
                CommandAction::SwitchLayout {
                    layout: LayoutId::new("en-US"),
                },
            ),
            (
                "open_path",
                CommandAction::OpenPath {
                    path: "C:/work/notes.md".into(),
                },
            ),
        ];
        for (tag, action) in cases {
            let toml_str = toml::to_string(&action).expect("serialise");
            assert!(
                toml_str.contains(&format!("type = \"{tag}\"")),
                "expected `type = \"{tag}\"` in serialised form, got: {toml_str}"
            );
            let back: CommandAction = toml::from_str(&toml_str).expect("parse");
            assert_eq!(action, back);
        }
    }

    /// A complete config snippet a user might write — array-of-tables
    /// `[[commands]]` parses end-to-end through the wrapper used in
    /// `Settings`. Triggers are TYPED text, not hotkey combos.
    #[test]
    fn parses_complete_user_command_block() {
        // We mirror the wrapper that `Settings.commands` will use:
        // a top-level `commands` field of type `Vec<UserCommand>`.
        #[derive(Deserialize)]
        struct Wrap {
            #[serde(default)]
            commands: Vec<UserCommand>,
        }
        let raw = r#"
[[commands]]
id      = "anrl"
name    = "Anatomical reference list"
trigger = "anrl"
action  = { type = "type_text", text = "Anatomical Reference List" }
apps    = ["thunderbird.exe", "OUTLOOK.EXE"]

[[commands]]
id      = "to-english"
trigger = "((en))"
action  = { type = "switch_layout", layout = "en-US" }
"#;
        let w: Wrap = toml::from_str(raw).expect("parse");
        assert_eq!(w.commands.len(), 2);

        let anrl = &w.commands[0];
        assert_eq!(anrl.id, "anrl");
        assert_eq!(anrl.name, "Anatomical reference list");
        assert_eq!(anrl.trigger, "anrl");
        assert!(matches!(
            &anrl.action,
            CommandAction::TypeText { text } if text == "Anatomical Reference List"
        ));
        assert_eq!(anrl.apps.len(), 2);

        let lang = &w.commands[1];
        assert_eq!(lang.id, "to-english");
        // `name` defaults to empty — UI falls back to `id` for display.
        assert!(lang.name.is_empty());
        assert!(lang.apps.is_empty());
        assert!(matches!(
            &lang.action,
            CommandAction::SwitchLayout { layout } if layout.as_str() == "en-US"
        ));
    }

    /// Reserved built-in ids must be exposed as constants — the UI
    /// validation layer consults them to refuse or warn when a
    /// user tries to shadow `pause-toggle` / `switch-last`.
    #[test]
    fn builtin_ids_are_distinct_and_kebab_case() {
        assert_ne!(BUILTIN_PAUSE_TOGGLE_ID, BUILTIN_SWITCH_LAST_ID);
        for id in [BUILTIN_PAUSE_TOGGLE_ID, BUILTIN_SWITCH_LAST_ID] {
            assert!(id.chars().all(|c| c.is_ascii_lowercase() || c == '-'));
        }
    }

    fn cmd(id: &str, trigger: &str, apps: &[&str]) -> UserCommand {
        UserCommand {
            id: id.into(),
            name: String::new(),
            trigger: trigger.into(),
            action: CommandAction::TypeText { text: "x".into() },
            apps: apps.iter().map(|s| (*s).into()).collect(),
        }
    }

    /// Trigger matching is exact, case-sensitive, and consults the
    /// optional `apps` filter. These three properties together are
    /// what users will lean on when designing triggers — getting
    /// any one of them wrong breaks expectations silently.
    #[test]
    fn find_matching_command_basic_match() {
        let list = vec![cmd("a", "anrl", &[]), cmd("b", "((en))", &[])];

        // Exact match.
        let m = find_matching_command(&list, "anrl", None).expect("matches");
        assert_eq!(m.id, "a");
        // Different trigger.
        let m2 = find_matching_command(&list, "((en))", None).expect("matches");
        assert_eq!(m2.id, "b");
        // No match — case-sensitive, "ANRL" != "anrl".
        assert!(find_matching_command(&list, "ANRL", None).is_none());
        // No match — completely unrelated word.
        assert!(find_matching_command(&list, "hello", None).is_none());
        // No match — empty word.
        assert!(find_matching_command(&list, "", None).is_none());
    }

    /// `apps` filter: empty list = match anywhere; non-empty = the
    /// focused app's basename must match (case-insensitive).
    #[test]
    fn find_matching_command_app_filter() {
        let list = vec![cmd("a", "anrl", &["Code.exe", "idea64.exe"])];

        // No app reported → fail closed (filter set, can't verify).
        assert!(find_matching_command(&list, "anrl", None).is_none());
        // Wrong app → no match.
        assert!(find_matching_command(&list, "anrl", Some("chrome.exe")).is_none());
        // Right app, exact case → match.
        assert!(find_matching_command(&list, "anrl", Some("Code.exe")).is_some());
        // Right app, wrong case → still match (case-insensitive
        // basename comparison, mirrors `disabled_apps` rules).
        assert!(find_matching_command(&list, "anrl", Some("CODE.EXE")).is_some());
    }

    /// First match wins when two commands share a trigger. That's
    /// a config error in practice — but we resolve it
    /// deterministically rather than crashing the engine.
    #[test]
    fn find_matching_command_first_match_wins() {
        let list = vec![cmd("a", "dup", &[]), cmd("b", "dup", &[])];
        let m = find_matching_command(&list, "dup", None).expect("matches");
        assert_eq!(m.id, "a");
    }
}
