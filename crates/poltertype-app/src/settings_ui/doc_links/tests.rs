//! What a catalog may and may not do to a link out of this window.

use super::*;
use crate::settings_ui::consts::PERMISSIONS_DOC_URL;

/// The one guide the Setup pane opens has to be a guide this module
/// knows how to translate, or the feature is wired to nothing.
#[test]
fn the_setup_guide_is_a_translatable_doc() {
    let file = PERMISSIONS_DOC_URL
        .strip_prefix(DOCS_PREFIX)
        .unwrap_or_default();
    assert!(
        TRANSLATED_DOCS.iter().any(|(name, _)| *name == file),
        "the Setup pane opens {PERMISSIONS_DOC_URL}, which this module cannot translate"
    );
}

#[test]
fn a_translated_guide_swaps_in_its_language() {
    assert_eq!(
        in_language("PERMISSIONS.md", "uk").as_deref(),
        Some("https://github.com/Just-Code-NET/PolterType/blob/main/docs/PERMISSIONS.uk.md")
    );
}

/// English is the file already named, not `PERMISSIONS.en.md`.
#[test]
fn english_stays_on_the_original_file() {
    assert_eq!(in_language("PERMISSIONS.md", "en"), None);
}

#[test]
fn a_script_or_region_tag_is_a_language() {
    assert!(in_language("PERMISSIONS.md", "pt-BR").is_some());
    assert!(in_language("PERMISSIONS.md", "zh-Hant-TW").is_some());
}

/// The whole point of holding the host in the program: none of these
/// may reach `opener`.
#[test]
fn a_catalog_cannot_aim_the_link_somewhere_else() {
    for hostile in [
        "../../../etc/passwd",
        "https://example.invalid/phish",
        "uk/../..",
        "a b",
        "",
        "a-very-long-not-a-language",
    ] {
        assert_eq!(
            in_language("PERMISSIONS.md", hostile),
            None,
            "`{hostile}` was accepted as a language"
        );
    }
}

/// Every other link the window opens — the site, the repository, a
/// macOS `x-apple.systempreferences:` deep link — passes through
/// untouched.
#[test]
fn a_link_that_is_not_a_guide_is_left_alone() {
    for url in [
        "https://poltertype.com",
        "https://github.com/Just-Code-NET/PolterType/issues",
        "https://github.com/Just-Code-NET/PolterType/blob/main/docs/PLAN.md",
        "x-apple.systempreferences:com.apple.preference.security",
    ] {
        assert_eq!(localized(url), url);
    }
}

/// End to end: the line a translator writes, through the catalog, to
/// the address the Setup pane's button opens.
///
/// The scratch catalog holds that one key and nothing else, so the
/// swap it performs — global, for the rest of this test binary — can
/// change no other test's words.
#[test]
fn a_catalog_line_moves_the_button_to_the_translation() {
    assert_eq!(
        localized(PERMISSIONS_DOC_URL),
        PERMISSIONS_DOC_URL,
        "with no catalog loaded the guide is the English one"
    );

    // Setup errors are ignored the way the i18n tests ignore theirs:
    // a directory that did not get written shows up as the assertion
    // below, not as a second failure mode to read past.
    let dir = std::env::temp_dir().join(format!("pt-doc-links-{}", std::process::id()));
    let _ = std::fs::create_dir_all(dir.join("i18n"));
    let _ = std::fs::write(
        dir.join("i18n").join("uk.toml"),
        "\"docs.permissions\" = \"uk\"\n",
    );
    poltertype_core::i18n::reload(&dir, Some("uk"), &[]);

    assert_eq!(
        localized(PERMISSIONS_DOC_URL),
        "https://github.com/Just-Code-NET/PolterType/blob/main/docs/PERMISSIONS.uk.md"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
