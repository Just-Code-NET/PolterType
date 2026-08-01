use super::*;

// ── locale resolution ────────────────────────────────────────────────

#[test]
fn an_explicit_setting_wins_over_the_environment() {
    assert_eq!(resolve_locale(Some("uk")), "uk");
    assert_eq!(resolve_locale(Some("pt-BR")), "pt_BR");
}

#[test]
fn auto_and_blank_defer_to_the_environment() {
    // Whatever this machine's env says, both must agree — the point
    // is that neither is treated as an explicit choice.
    assert_eq!(resolve_locale(Some("auto")), resolve_locale(None));
    assert_eq!(resolve_locale(Some("  ")), resolve_locale(None));
    assert_eq!(resolve_locale(Some("AUTO")), resolve_locale(None));
}

#[test]
fn encodings_and_modifiers_are_stripped() {
    assert_eq!(resolve_locale(Some("uk_UA.UTF-8")), "uk_UA");
    assert_eq!(resolve_locale(Some("de_DE@euro")), "de_DE");
    assert_eq!(resolve_locale(Some("EL_gr.utf8")), "el_GR");
}

// ── catalogs ─────────────────────────────────────────────────────────

fn catalog(body: &str) -> Catalog {
    Catalog::parse("uk", body, "<test>")
}

#[test]
fn a_catalog_translates_known_keys() {
    let c = catalog(
        r#"
"languages.title" = "Мови"
"general.save" = "Зберегти"
"#,
    );
    assert_eq!(c.get("languages.title"), Some("Мови"));
    assert_eq!(c.get("general.save"), Some("Зберегти"));
    assert_eq!(c.get("nothing.here"), None);
    assert_eq!(c.len(), 2);
}

/// The property the whole design rests on: anything wrong with a
/// catalog degrades to English, never to a blank or a raw key.
#[test]
fn a_broken_catalog_is_empty_rather_than_wrong() {
    for body in [
        "this is not toml at all {{{",
        "[nested]\nkey = \"x\"", // tables are skipped, not adopted
        "count = 3",             // non-string values
        "",
    ] {
        let c = catalog(body);
        assert!(
            c.get("count").is_none() && c.get("key").is_none(),
            "nothing usable should come out of {body:?}"
        );
    }
}

/// An empty string is "not translated yet". Storing it would shadow
/// the English fallback with a blank label — the one result worse
/// than staying untranslated.
#[test]
fn empty_translations_do_not_shadow_the_english() {
    let c = catalog(
        r#"
"a" = ""
"b" = "   "
"c" = "справжній"
"#,
    );
    assert_eq!(c.get("a"), None);
    assert_eq!(c.get("b"), None);
    assert_eq!(c.get("c"), Some("справжній"));
}

/// One bad entry costs that entry, not the language — the same rule
/// the AI plug-in factory follows for one bad `[[ai.plugins]]` block.
#[test]
fn one_bad_entry_does_not_discard_the_good_ones() {
    let c = catalog(
        r#"
"good" = "добре"
"numeric" = 42
"alsogood" = "теж"
"#,
    );
    assert_eq!(c.get("good"), Some("добре"));
    assert_eq!(c.get("alsogood"), Some("теж"));
    assert_eq!(c.get("numeric"), None);
}

#[test]
fn a_missing_directory_yields_an_empty_catalog() {
    let c = Catalog::load(std::path::Path::new("/nonexistent/poltertype/i18n"), "uk");
    assert!(c.is_empty());
    assert_eq!(c.locale(), "uk");
}

/// `uk_UA` should find `uk.toml`: shipping one file per region would
/// be a lot of duplication for no benefit.
#[test]
fn a_regional_locale_falls_back_to_the_bare_language() {
    let dir = std::env::temp_dir().join(format!("pt-i18n-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("uk.toml"), "\"k\" = \"значення\"\n");

    let c = Catalog::load(&dir, "uk_UA");
    assert_eq!(c.get("k"), Some("значення"), "uk_UA must find uk.toml");

    let _ = std::fs::remove_dir_all(&dir);
}

// ── the public entry point ───────────────────────────────────────────

/// `tr` must be safe to call before `init`, because a panic in the
/// view function would take the settings window down.
#[test]
fn tr_falls_back_to_english_when_uninitialised() {
    assert_eq!(tr("some.key.no.one.set", "English text"), "English text");
}

#[test]
fn shipped_locales_are_well_formed() {
    for (code, name) in SHIPPED_LOCALES {
        assert!(!code.is_empty() && !name.is_empty());
        assert_eq!(*code, code.to_ascii_lowercase(), "codes are lowercase");
        assert!(
            !code.starts_with("en"),
            "English is the fallback, not a catalog"
        );
    }
}

// ── placeholder substitution ─────────────────────────────────────────

#[test]
fn placeholders_are_filled_in_order() {
    assert_eq!(
        tr_args("k", "Restricted to {} of {} layouts", &["2", "15"]),
        "Restricted to 2 of 15 layouts"
    );
}

#[test]
fn a_string_without_placeholders_is_returned_as_is() {
    assert_eq!(
        tr_args("k", "No placeholders here", &[]),
        "No placeholders here"
    );
}

/// Neither mismatch may panic: this runs inside the view function, and
/// a panic there takes the settings window down over a typo in a
/// community translation.
#[test]
fn placeholder_count_mismatches_are_survivable() {
    // More arguments than slots: a translation that legitimately drops
    // one is honoured as written.
    assert_eq!(
        tr_args("k", "Only {} shown", &["3", "unused"]),
        "Only 3 shown"
    );
    // More slots than arguments: the extra stays visible rather than
    // silently swallowing text around it.
    assert_eq!(tr_args("k", "{} of {}", &["3"]), "3 of {}");
    assert_eq!(tr_args("k", "{}{}{}", &[]), "{}{}{}");
}

#[test]
fn braces_that_are_not_placeholders_survive() {
    assert_eq!(
        tr_args("k", "Use {braces} and {} here", &["this"]),
        "Use {braces} and this here"
    );
}

// ── the shipped catalog ──────────────────────────────────────────────

/// The Ukrainian catalog in `data/i18n/` must parse and actually
/// translate. Reads the repository file directly rather than the
/// built dist tree, so it fails on a bad edit even before a build
/// copies anything.
#[test]
fn the_shipped_ukrainian_catalog_is_usable() {
    let repo_file =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/i18n/uk.toml");
    let Ok(text) = std::fs::read_to_string(&repo_file) else {
        // A consumer building from a package without `data/` — not a
        // failure of this crate.
        return;
    };
    let c = Catalog::parse("uk", &text, "uk.toml");
    assert!(c.len() > 40, "catalog looks truncated: {} entries", c.len());

    // Spot-check one label per pane, so a wholesale key rename in the
    // UI shows up here instead of as a silently English window.
    for key in [
        "ui.settings",
        "languages.languages",
        "hotkeys.hotkeys",
        "commands.commands",
        "wordlists.wordlists",
        "general.general",
        "suggestions.suggestions",
        "exceptions.exceptions",
        "setup.setup",
    ] {
        let value = c.get(key);
        assert!(value.is_some(), "missing translation for `{key}`");
        assert!(
            value.is_some_and(|v| v.chars().any(|ch| ('\u{0400}'..'\u{04FF}').contains(&ch))),
            "`{key}` should be Cyrillic, got {value:?}"
        );
    }
}

/// Placeholder counts have to survive translation, or a sentence
/// loses the number it was built around.
#[test]
fn the_ukrainian_catalog_keeps_its_placeholders() {
    let repo_file =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/i18n/uk.toml");
    let Ok(text) = std::fs::read_to_string(&repo_file) else {
        return;
    };
    let c = Catalog::parse("uk", &text, "uk.toml");
    if let Some(v) = c.get("languages.status_restricted") {
        assert!(v.contains("{}"), "the layout count must survive: {v}");
    }
}
