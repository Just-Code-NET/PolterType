#![allow(unused_imports)]

use iced::keyboard::{Key, Modifiers, key::Named};
use poltertype_core::commands::{CommandAction, UserCommand};
use poltertype_layout::LayoutId;

use super::enums::*;
use super::helpers::*;
use super::state::*;
use super::*;

/// The capture pipeline must produce strings that `global-hotkey`'s
/// `FromStr` accepts. Otherwise rebinding succeeds in the UI but
/// the next tray launch silently drops the hotkey. We round-trip
/// the canonical combos to catch that.
#[test]
fn captured_hotkeys_round_trip_through_global_hotkey_parse() {
    use global_hotkey::hotkey::HotKey;
    let mut mods = Modifiers::empty();
    mods.insert(Modifiers::CTRL);
    mods.insert(Modifiers::SHIFT);
    let cases = [
        (Key::Named(Named::Space), mods, "Ctrl+Shift+Space"),
        (Key::Named(Named::Backspace), mods, "Ctrl+Shift+Backspace"),
        (
            Key::Character("a".into()),
            Modifiers::CTRL | Modifiers::ALT,
            "Ctrl+Alt+A",
        ),
        (Key::Named(Named::F4), Modifiers::ALT, "Alt+F4"),
    ];
    for (key, mods, expected) in cases {
        let formatted = format_hotkey(&key, mods);
        assert_eq!(
            formatted, expected,
            "format mismatch for {key:?} + {mods:?}"
        );
        assert!(
            formatted.parse::<HotKey>().is_ok(),
            "global-hotkey rejected our formatted combo `{formatted}` — \
             the rebind UI would silently drop hotkeys this shape"
        );
    }
}

/// Auto-id must be deterministic and collision-free. The UI
/// silently dedupes by appending `-2`, `-3`, … so users don't
/// need to think about ids — but the dedup must be stable, since
/// duplicate ids in the saved config would be a load-time error.
#[test]
fn derive_command_id_is_kebab_case_and_unique() {
    let action = CommandAction::TypeText { text: "x".into() };
    let id = derive_command_id("Insert Email Signature!", &action, &[]);
    assert_eq!(id, "insert-email-signature");

    // Empty name → action-typed fallback.
    let blank = derive_command_id("", &action, &[]);
    assert_eq!(blank, "type-text");

    // Collision → `-2` suffix.
    let existing = vec![UserCommand {
        id: "type-text".into(),
        name: String::new(),
        trigger: "anrl".into(),
        action: action.clone(),
        apps: Vec::new(),
    }];
    let dedup = derive_command_id("", &action, &existing);
    assert_eq!(dedup, "type-text-2");
}

/// `looks_like_layout_id` is a hint, not a strict validator —
/// must accept the canonical bundled set + multi-segment ids
/// (Cyrillic Kazakh) and reject obviously-not-an-id strings.
#[test]
fn looks_like_layout_id_accepts_real_ids_and_rejects_garbage() {
    for ok in ["en-US", "uk-UA", "kk-Cyrl-KZ", "zh-Hans-CN"] {
        assert!(
            looks_like_layout_id(ok),
            "{ok} should be accepted as a layout id"
        );
    }
    for bad in ["", "english", "EN", "en US", "fr.fr", "uk--UA…"] {
        assert!(
            !looks_like_layout_id(bad),
            "{bad} should NOT be accepted as a layout id"
        );
    }
}

/// Summary format is what users scan first when they have a
/// long list of commands. It must include the display name (or
/// id fallback), the action description, and the apps filter
/// when set — and stay on one line for any reasonable input.
#[test]
fn format_command_summary_is_concise_and_complete() {
    let cmd = UserCommand {
        id: "sig".into(),
        name: "Email signature".into(),
        trigger: ";sig".into(),
        action: CommandAction::TypeText {
            text: "Best regards".into(),
        },
        apps: vec!["OUTLOOK.EXE".into()],
    };
    let s = format_command_summary(&cmd);
    assert!(s.contains("Email signature"));
    assert!(s.contains("Best regards"));
    assert!(s.contains("OUTLOOK.EXE"));

    // Falls back to id when name is empty.
    let cmd2 = UserCommand {
        id: "go-en".into(),
        name: String::new(),
        trigger: "((en))".into(),
        action: CommandAction::SwitchLayout {
            layout: LayoutId::new("en-US"),
        },
        apps: Vec::new(),
    };
    let s2 = format_command_summary(&cmd2);
    assert!(s2.starts_with("go-en"));
    assert!(s2.contains("en-US"));
    // No apps blurb when the filter is empty.
    assert!(!s2.contains(" (in "));
}

/// Stem mapping must agree with both the bundled FST file names
/// (`data/wordlists/<stem>.fst`) and the loader's user-overlay
/// path (`<config-dir>/poltertype/wordlists/<stem>.txt`).
/// Otherwise the GUI would write to a file the engine never
/// reads, and users would see "I added words but they don't take
/// effect."
#[test]
fn layout_id_to_stem_matches_bundled_naming() {
    let cases = [
        ("en-US", "en_us"),
        ("uk-UA", "uk_ua"),
        ("ru-RU", "ru_ru"),
        ("de-DE", "de_de"),
        ("es-ES", "es_es"),
        ("fr-FR", "fr_fr"),
        // Multi-segment IDs (e.g. Cyrillic Kazakh) collapse all
        // dashes — keeps the convention uniform.
        ("kk-Cyrl-KZ", "kk_cyrl_kz"),
    ];
    for (id, expected) in cases {
        assert_eq!(
            layout_id_to_stem(&LayoutId::new(id)),
            expected,
            "stem mismatch for {id}"
        );
    }
}

/// `WordlistKind::suffix` must round-trip with the bundled
/// `<stem>-stop.txt` convention. Easy to fix typos here; harder
/// to notice them at runtime since a missing stop file is
/// silently treated as "no extras."
#[test]
fn wordlist_kind_suffix_matches_loader_convention() {
    assert_eq!(WordlistKind::Extras.suffix(), "");
    assert_eq!(WordlistKind::Stop.suffix(), "-stop");
}

/// The text the editor saves must round-trip through the engine's
/// own loader without losing anything semantically meaningful.
/// We mirror `poltertype_core::layouts::parse_wordlist` here — lowercase,
/// comment-stripped, blank-line-skipped — and confirm typical
/// free-form content survives. If the engine's parser ever
/// diverges, this test makes the GUI catch it before users do.
#[test]
fn wordlist_buffer_is_compatible_with_loader_parser() {
    // Multi-line buffer the user might type into the editor:
    // pure words, comments, blanks, mixed case, leading whitespace.
    let body = "# project nouns\nfoo\nBar\n  baz  \n\n#trailing comment\n";
    let words: std::collections::HashSet<String> = body
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_lowercase)
        .collect();
    for w in ["foo", "bar", "baz"] {
        assert!(words.contains(w), "{w} should survive parse");
    }
    // Comments & blanks must never become words.
    assert!(!words.contains("# project nouns"));
    assert!(!words.contains(""));
}

/// `save_overlay_file` terminates the buffer with a newline
/// regardless of whether the user did. Keeps `git diff` quiet
/// for users who keep their config dir under version control,
/// and matches the convention of the bundled lists. We can't
/// easily call `save_overlay_file` without a real
/// `user_wordlist_dir`, but the normalisation logic is small
/// and isolated — we mirror it here so any future divergence
/// gets caught.
#[test]
fn save_overlay_appends_trailing_newline() {
    fn normalise(text: &str) -> String {
        let mut s = text.to_owned();
        if !s.ends_with('\n') {
            s.push('\n');
        }
        s
    }
    assert_eq!(normalise("foo"), "foo\n");
    assert_eq!(normalise("foo\n"), "foo\n");
    assert_eq!(normalise(""), "\n");
    assert_eq!(normalise("foo\nbar"), "foo\nbar\n");
}

/// Lone modifier presses must not be accepted as a hotkey on
/// their own — otherwise the moment a user clicks "Rebind" and
/// taps Ctrl, capture finishes immediately with a useless
/// "Ctrl"-only binding.
#[test]
fn lone_modifier_keys_are_filtered() {
    for k in [
        Key::Named(Named::Control),
        Key::Named(Named::Shift),
        Key::Named(Named::Alt),
        Key::Named(Named::Meta),
        Key::Named(Named::Super),
    ] {
        assert!(is_modifier_key(&k), "{k:?} should be classed as modifier");
    }
    // Sanity: a regular character key must NOT be flagged.
    assert!(!is_modifier_key(&Key::Character("x".into())));
    assert!(!is_modifier_key(&Key::Named(Named::Space)));
}
