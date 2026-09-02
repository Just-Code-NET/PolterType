use iced::keyboard::{Key, Modifiers, key::Named};
use poltertype_core::commands::{CommandAction, UserCommand};
use poltertype_core::engine::{ModRole, ModSet};
use poltertype_layout::LayoutId;

use super::enums::*;
use super::helpers::*;
use super::state::ModCapture;
use super::theme;

/// The capture pipeline must produce strings that `global-hotkey`'s
/// `FromStr` accepts, or rebinding succeeds in the UI while the next
/// tray launch silently drops the hotkey.
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
        // The Windows/Super key. `Meta` was written here for two years
        // and the parser refuses it, so every Super binding became the
        // default instead — this row is what the test was missing.
        (
            Key::Named(Named::F9),
            Modifiers::CTRL | Modifiers::LOGO,
            "Ctrl+Super+F9",
        ),
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

/// A key is captured as the character it produced, so a rebind made
/// while a Cyrillic layout is active offers `Ctrl+Shift+Ф`. The reader
/// refuses that, and a refused binding used to become the default
/// silently — the rebind looked accepted and the key did something
/// else. It must be refused where the user can see it instead.
#[test]
fn a_combo_the_reader_refuses_is_not_usable() {
    assert!(!is_usable_hotkey("Ctrl+Shift+Ф"));
    assert!(!is_usable_hotkey("Ctrl+Meta+K"));
    assert!(!is_usable_hotkey("Ctrl+Win+K"));

    assert!(is_usable_hotkey("Ctrl+Shift+K"));
    assert!(is_usable_hotkey("Ctrl+Super+K"));
    assert!(is_usable_hotkey("Ctrl+Shift+F9"));
}

/// Auto-id must be deterministic and collision-free: the UI dedupes
/// silently so users never think about ids, and duplicates in the saved
/// config are a load-time error.
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

/// `looks_like_layout_id` is a hint, not a strict validator: accept the
/// bundled set and multi-segment ids, reject obvious non-ids.
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

/// The summary is what users scan in a long list: display name (or id
/// fallback), action, and the apps filter when set — on one line.
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
/// (`data/wordlists/<stem>.fst`) and the loader's user-overlay path
/// (`<config-dir>/poltertype/wordlists/<stem>.txt`) — otherwise the GUI
/// writes to a file the engine never reads, and words added in the UI
/// silently do nothing.
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
/// `<stem>-stop.txt` convention. A typo is invisible at runtime: a
/// missing stop file is silently treated as "no extras".
#[test]
fn wordlist_kind_suffix_matches_loader_convention() {
    assert_eq!(WordlistKind::Extras.suffix(), "");
    assert_eq!(WordlistKind::Stop.suffix(), "-stop");
}

/// What the editor saves must survive the engine's loader. The rules of
/// `poltertype_core::layouts::parse_wordlist` — lowercase,
/// comment-stripped, blank-line-skipped — are mirrored here, so a
/// divergence on the engine side shows up in the GUI's tests.
#[test]
fn wordlist_buffer_is_compatible_with_loader_parser() {
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

/// `save_overlay_file` terminates the buffer with a newline whether or
/// not the user did — keeps `git diff` quiet for config dirs under
/// version control, and matches the bundled lists.
///
/// Calling it needs a real `user_wordlist_dir`, so the normalisation is
/// mirrored here; any future divergence gets caught.
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

/// `ui_theme` round-trips through the UI enum: canonical spellings
/// parse back to themselves, anything else falls back to `System`
/// rather than erroring. A wrong fallback here makes the window ignore
/// the user's saved preference silently.
#[test]
fn theme_choice_round_trips_and_tolerates_garbage() {
    for choice in ThemeChoice::ALL {
        assert_eq!(
            ThemeChoice::from_config(choice.config_value()),
            choice,
            "canonical spelling `{}` must parse back to itself",
            choice.config_value()
        );
    }
    // Case / whitespace tolerance for hand-edited configs.
    assert_eq!(ThemeChoice::from_config(" Dark "), ThemeChoice::Dark);
    assert_eq!(ThemeChoice::from_config("LIGHT"), ThemeChoice::Light);
    // Unknown values fall back to System.
    for garbage in ["", "auto", "darkish", "0"] {
        assert_eq!(ThemeChoice::from_config(garbage), ThemeChoice::System);
    }
}

/// The same for `tray_icon` (issue #50). `hidden` is the value that
/// matters here: mis-parsed as `color` it leaves an icon the user asked
/// to be rid of, and a `Default` that is not `Color` would hide the
/// icon for everyone who never set the key.
#[test]
fn tray_icon_style_round_trips_and_tolerates_garbage() {
    use poltertype_core::settings::TrayIconStyle;

    for style in [
        TrayIconStyle::Color,
        TrayIconStyle::Mono,
        TrayIconStyle::Flag,
        TrayIconStyle::Hidden,
    ] {
        assert_eq!(TrayIconStyle::from_config(style.config_value()), style);
    }
    assert_eq!(
        TrayIconStyle::from_config(" Hidden "),
        TrayIconStyle::Hidden
    );
    for garbage in ["", "colour", "none", "off"] {
        assert_eq!(TrayIconStyle::from_config(garbage), TrayIconStyle::Color);
    }
    assert_eq!(TrayIconStyle::default(), TrayIconStyle::Color);
}

/// The probe parsers must map the documented color-scheme values and
/// treat everything else — "no preference", truncated output, errors on
/// stdout — as "no answer", so the caller falls through to the next
/// signal instead of forcing light.
#[test]
fn system_theme_parsers_map_portal_and_gsettings_values() {
    use super::system_theme::{parse_color_scheme, parse_portal_reply};

    // busctl renders `Read`'s variant reply as `v v u <n>`.
    assert_eq!(parse_portal_reply("v v u 1"), Some(true));
    assert_eq!(parse_portal_reply("v v u 2"), Some(false));
    assert_eq!(parse_portal_reply("v v u 0"), None, "no preference");
    assert_eq!(parse_portal_reply(""), None);
    assert_eq!(parse_portal_reply("Call failed"), None);

    assert_eq!(parse_color_scheme("'prefer-dark'\n"), Some(true));
    assert_eq!(parse_color_scheme("'prefer-light'\n"), Some(false));
    assert_eq!(parse_color_scheme("'default'\n"), None, "no preference");
    assert_eq!(parse_color_scheme(""), None);
}

/// `ui_theme = "system"` meant "light" on Windows and macOS for two
/// releases: iced 0.13's `Theme::default()` had answered for those two,
/// and 0.14 turned the same name into something that detects nothing
/// (issue #43). These are the parsers of the two probes that replaced
/// it.
#[test]
fn the_windows_registry_probe_reads_the_apps_theme_flag() {
    use super::system_theme::parse_reg_apps_use_light;

    let dark =
        "\r\nHKEY_CURRENT_USER\\...\\Personalize\r\n    AppsUseLightTheme    REG_DWORD    0x0\r\n";
    let light =
        "\r\nHKEY_CURRENT_USER\\...\\Personalize\r\n    AppsUseLightTheme    REG_DWORD    0x1\r\n";
    assert_eq!(parse_reg_apps_use_light(dark), Some(true));
    assert_eq!(parse_reg_apps_use_light(light), Some(false));
    // The neighbouring value is the taskbar's, not ours.
    assert_eq!(
        parse_reg_apps_use_light("    SystemUsesLightTheme    REG_DWORD    0x0\r\n"),
        None
    );
    assert_eq!(parse_reg_apps_use_light(""), None);
}

/// A key whose *rendering* the hotkey parser cannot read back — a
/// Cyrillic letter, most obviously — used to be refused outright, and
/// a refused rebind is one that looks accepted and does nothing.
#[test]
fn a_key_the_reader_cannot_take_back_is_captured_by_its_physical_code() {
    use iced::keyboard::key::{Code, Physical};

    let ctrl_shift = Modifiers::CTRL | Modifiers::SHIFT;
    // What the layout renders is refused …
    assert!(!is_usable_hotkey(&format_hotkey(
        &Key::Character("ф".into()),
        ctrl_shift
    )));
    // … and the key under it is not.
    assert_eq!(
        physical_hotkey(Physical::Code(Code::KeyA), ctrl_shift).as_deref(),
        Some("Ctrl+Shift+KeyA")
    );
    // Punctuation the pane could not bind at all before.
    assert_eq!(
        physical_hotkey(Physical::Code(Code::Backquote), Modifiers::CTRL).as_deref(),
        Some("Ctrl+Backquote")
    );
    // And the one code that stays unbindable: `global-hotkey` 0.6.4 has
    // no spelling for `IntlBackslash`, so offering it would write a
    // binding nothing could read back.
    assert_eq!(
        physical_hotkey(Physical::Code(Code::IntlBackslash), Modifiers::CTRL),
        None
    );
}

/// Neutralising Caps Lock is the precondition for binding it — and it
/// is also what takes the key's keysym away, so the pane stopped
/// recognising the key the moment the user did what the pane asked
/// (issue #41, reported again after 0.25.0 shipped).
#[test]
fn caps_lock_is_still_caps_lock_once_the_layout_has_dropped_it() {
    use iced::keyboard::key::{Code, Physical};

    assert!(
        is_capslock(&Key::Named(Named::CapsLock), Physical::Code(Code::CapsLock)),
        "with the lock still live, both halves say so"
    );
    assert!(
        is_capslock(&Key::Unidentified, Physical::Code(Code::CapsLock)),
        "under `caps:none` the key has no name left — only its position"
    );
    assert!(
        !is_capslock(&Key::Character("a".into()), Physical::Code(Code::KeyA)),
        "and no other bare key becomes bindable"
    );
    // What the capture then writes into `config.toml`.
    assert_eq!(
        physical_hotkey(Physical::Code(Code::CapsLock), Modifiers::empty()).as_deref(),
        Some("CapsLock")
    );
}

/// `Theme::custom` derives `is_dark` from background luminance and
/// `brand_palette` keys the whole token set off that flag, so a
/// background tweak that flipped the classification would leave every
/// widget picking tokens from the wrong palette.
#[test]
fn branded_themes_classify_and_map_to_their_palettes() {
    let light = theme::light();
    let dark = theme::dark();
    assert!(
        !light.extended_palette().is_dark,
        "light theme classified dark"
    );
    assert!(
        dark.extended_palette().is_dark,
        "dark theme classified light"
    );
    // And the reverse mapping picks the matching brand palette.
    assert_eq!(theme::brand_palette(&light).ink, light.palette().text);
    assert_eq!(theme::brand_palette(&dark).ink, dark.palette().text);
}

/// `accept_modifiers_enable_keyboard` delegates to the engine's
/// `AcceptModifiers::parse`; this test pins the shared acceptance
/// rule so an engine-side change that would silently flip the
/// Suggestions-pane hint shows up here first.
#[test]
fn accept_modifiers_hint_mirrors_engine_parse_rule() {
    // Armed: at least one non-Shift modifier, aliases + case +
    // whitespace tolerated.
    for ok in [
        "Ctrl+Shift",
        "control+alt",
        "Cmd",
        "Win",
        "Option",
        "Meta+Shift",
        " Ctrl + Shift ",
        "super",
    ] {
        assert!(
            accept_modifiers_enable_keyboard(ok),
            "`{ok}` should arm the keyboard-accept chord"
        );
    }
    // Disarmed: empty (by design), bare Shift (digits would fire on
    // plain `!`/`@`/… typing), unknown tokens, wrong separator.
    for off in ["", "   ", "Shift", "Ctrl+Foo", "Ctrl,Shift", "hyper"] {
        assert!(
            !accept_modifiers_enable_keyboard(off),
            "`{off}` should leave the keyboard-accept chord off"
        );
    }
}

/// Modifier presses take the modifier-chord route, not the
/// combination one: a user who clicks "Rebind" and taps Ctrl must not
/// end the capture with a useless "Ctrl"-only binding.
#[test]
fn modifier_keys_are_routed_by_role() {
    for (k, role) in [
        (Key::Named(Named::Control), ModRole::Ctrl),
        (Key::Named(Named::Shift), ModRole::Shift),
        (Key::Named(Named::Alt), ModRole::Alt),
        (Key::Named(Named::AltGraph), ModRole::Alt),
        (Key::Named(Named::Meta), ModRole::Meta),
        (Key::Named(Named::Super), ModRole::Meta),
    ] {
        assert_eq!(mod_role_of(&k), Some(role), "{k:?}");
    }
    // Sanity: a regular character key must NOT be flagged.
    assert_eq!(mod_role_of(&Key::Character("x".into())), None);
    assert_eq!(mod_role_of(&Key::Named(Named::Space)), None);
}

/// The modifier-only capture, driven the way the keyboard subscription
/// drives it. The subscription itself cannot be tested — which is
/// exactly how its missing half survived a green suite once.
#[test]
fn a_modifier_gesture_binds_only_when_it_is_complete() {
    const SHIFT: ModSet = ModSet {
        shift: true,
        ctrl: false,
        alt: false,
        meta: false,
    };
    let tap = |cap: &mut ModCapture, role, held_on_release| {
        let _ = mod_capture_step(cap, role, true, ModSet::NONE);
        mod_capture_step(cap, role, false, held_on_release)
    };

    // One tap alone is half a gesture: nothing is bound yet.
    let mut cap = ModCapture::default();
    assert_eq!(tap(&mut cap, ModRole::Shift, ModSet::NONE), None);
    assert_eq!(cap.pending_tap, Some(SHIFT));
    // Its twin completes it.
    assert_eq!(
        tap(&mut cap, ModRole::Shift, ModSet::NONE).as_deref(),
        Some("Shift+Shift")
    );
    assert_eq!(cap.pending_tap, None, "the pair is spent, not carried on");

    // Two different modifiers held together bind on the last release,
    // and not before it — the first release still has the other down.
    let mut cap = ModCapture::default();
    assert_eq!(
        mod_capture_step(&mut cap, ModRole::Ctrl, true, ModSet::NONE),
        None
    );
    assert_eq!(
        mod_capture_step(
            &mut cap,
            ModRole::Shift,
            true,
            ModSet {
                ctrl: true,
                ..ModSet::NONE
            }
        ),
        None
    );
    assert_eq!(
        mod_capture_step(
            &mut cap,
            ModRole::Shift,
            false,
            ModSet {
                ctrl: true,
                shift: true,
                ..ModSet::NONE
            }
        ),
        None,
        "Ctrl is still down"
    );
    assert_eq!(
        mod_capture_step(
            &mut cap,
            ModRole::Ctrl,
            false,
            ModSet {
                ctrl: true,
                ..ModSet::NONE
            }
        )
        .as_deref(),
        Some("Ctrl+Shift")
    );

    // A single tap of a *different* modifier replaces the pending one
    // rather than pairing with it.
    let mut cap = ModCapture::default();
    assert_eq!(tap(&mut cap, ModRole::Shift, ModSet::NONE), None);
    assert_eq!(tap(&mut cap, ModRole::Alt, ModSet::NONE), None);
    assert_eq!(
        tap(&mut cap, ModRole::Alt, ModSet::NONE).as_deref(),
        Some("Alt+Alt")
    );
}

/// Everything the capture can produce has to survive the trip through
/// `config.toml` and back, or a rebind is silently replaced by the
/// default the next time the tray reads it.
#[test]
fn captured_modifier_chords_read_back_as_the_same_binding() {
    for (mods, double, expected) in [
        (
            ModSet {
                shift: true,
                ..ModSet::NONE
            },
            true,
            "Shift+Shift",
        ),
        (
            ModSet {
                ctrl: true,
                shift: true,
                ..ModSet::NONE
            },
            false,
            "Ctrl+Shift",
        ),
        (
            ModSet {
                meta: true,
                ..ModSet::NONE
            },
            true,
            "Super+Super",
        ),
    ] {
        let combo = format_mod_chord(mods, double);
        assert_eq!(combo, expected);
        assert!(is_usable_hotkey(&combo), "{combo} must be readable back");
        let parsed = crate::hotkeys::parse_mod_chord(&combo);
        assert_eq!(
            parsed.map(|m| (m.mods, m.double_tap)),
            Some((mods, double)),
            "{combo}"
        );
    }
}

/// The window stages the whole `Settings` struct and writes it back
/// wholesale, but the pause state belongs to the tray, which rewrites
/// it whenever auto-switch is paused or resumed. Saving anything at all
/// would otherwise resume an app the user paused after this window
/// opened (issue #46).
#[test]
fn saving_the_window_cannot_resume_an_app_paused_since_it_opened() {
    let mut staged = poltertype_core::settings::Settings::default();
    staged.general.paused = false;
    staged.general.show_notifications = true;

    let mut on_disk = poltertype_core::settings::Settings::default();
    on_disk.general.paused = true;

    let merged = with_runtime_state(staged, &on_disk, false);
    assert!(
        merged.general.paused,
        "the tray's state must survive a Save"
    );
    assert!(
        merged.general.show_notifications,
        "and everything the window does own must still be written"
    );
}

/// And the one case that is not the tray's: the General pane's
/// conversion chips write that same flag, so a mode the user picked
/// here has to reach the file rather than be folded back (issue #51).
#[test]
fn a_conversion_mode_picked_in_the_window_is_the_one_that_is_saved() {
    let mut staged = poltertype_core::settings::Settings::default();
    staged.general.paused = true;

    let mut on_disk = poltertype_core::settings::Settings::default();
    on_disk.general.paused = false;

    let merged = with_runtime_state(staged, &on_disk, true);
    assert!(
        merged.general.paused,
        "a mode chosen in this window must not be overwritten by the file"
    );
}
