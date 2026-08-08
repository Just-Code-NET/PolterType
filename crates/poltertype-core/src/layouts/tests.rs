use std::path::PathBuf;

use poltertype_types::{LayoutId, OsKeymap, WordKey};

use super::consts::*;
use super::helpers::*;
use super::types::*;
use super::*;

#[test]
fn embedded_layouts_load() {
    let db = LayoutDb::load_embedded();
    for id in [
        "en-US", "uk-UA", "ru-RU", "de-DE", "es-ES", "fr-FR", "pl-PL", "cs-CZ", "el-GR", "he-IL",
        "tr-TR", "bg-BG", "it-IT", "pt-PT", "pt-BR",
    ] {
        assert!(
            db.get(&LayoutId::from(id)).is_some(),
            "embedded layout `{id}` did not load"
        );
    }
}

/// `build.rs::LAYOUTS` and [`BUNDLED_LAYOUT_STEMS`] are two lists that
/// have to agree; when they drift the runtime only *warns*, so nothing
/// fails until a user notices their language is missing. Pin the count
/// against what actually loads.
#[test]
fn every_bundled_stem_resolves_to_a_layout() {
    let db = LayoutDb::load_embedded();
    assert_eq!(
        db.len(),
        BUNDLED_LAYOUT_STEMS.len(),
        "{} bundled stems but {} layouts loaded — build.rs::LAYOUTS and \
         BUNDLED_LAYOUT_STEMS have drifted, or a TOML failed to parse",
        BUNDLED_LAYOUT_STEMS.len(),
        db.len()
    );
}

/// The active-filter feature: only requested layouts enter memory.
/// This is what saves RAM for users who don't have all fifteen
/// bundled languages installed in the OS — which is everyone.
#[test]
fn active_filter_drops_unrequested_layouts() {
    let want = [LayoutId::from("en-US"), LayoutId::from("uk-UA")];
    let db = LayoutDb::load(LoadOptions {
        active_filter: Some(&want),
        ..Default::default()
    })
    .expect("load with filter");
    assert!(db.get(&LayoutId::from("en-US")).is_some());
    assert!(db.get(&LayoutId::from("uk-UA")).is_some());
    // The rest are bundled-but-filtered → must NOT be in the DB.
    for skipped in ["ru-RU", "de-DE", "es-ES", "fr-FR"] {
        assert!(
            db.get(&LayoutId::from(skipped)).is_none(),
            "filter must keep `{skipped}` out of memory"
        );
    }
}

/// `peek_layout_id` is the fast pre-parse used by the active-
/// filter — must round-trip every shape of `id =` line we
/// actually emit in our TOMLs (double-quoted + spaces) plus the
/// shapes a hand-written user TOML might use (single-quoted, no
/// space around `=`).
#[test]
fn peek_layout_id_recognises_every_shape() {
    assert_eq!(
        peek_layout_id("id = \"en-US\"\nname = \"English\""),
        Some("en-US".into())
    );
    assert_eq!(peek_layout_id("id=\"uk-UA\""), Some("uk-UA".into()));
    assert_eq!(peek_layout_id("id = 'ru-RU'"), Some("ru-RU".into()));
    // Comments and blank lines must not derail the search.
    assert_eq!(
        peek_layout_id("# heading\n\nid = \"de-DE\""),
        Some("de-DE".into())
    );
    assert_eq!(peek_layout_id("name = \"only\""), None);
}

#[test]
fn letter_in_any_layout_is_shift_aware() {
    let db = LayoutDb::load_embedded();
    assert!(db.is_letter_in_any_layout(0x0C, false));
    assert!(!db.is_letter_in_any_layout(0x0C, true));
}

#[test]
fn new_languages_translate_distinctive_keys() {
    let db = LayoutDb::load_embedded();
    let cases = [
        ("ru-RU", 0x10u32, false, 'й'),
        ("ru-RU", 0x29, false, 'ё'),
        ("de-DE", 0x15, false, 'z'),
        ("de-DE", 0x2C, false, 'y'),
        ("de-DE", 0x1A, false, 'ü'),
        ("es-ES", 0x27, false, 'ñ'),
        ("fr-FR", 0x10, false, 'a'),
        ("fr-FR", 0x03, false, 'é'),
        // Czech puts letters on the unshifted number row and swaps
        // Y/Z like German.
        ("cs-CZ", 0x03, false, 'ě'),
        ("cs-CZ", 0x15, false, 'z'),
        ("cs-CZ", 0x2C, false, 'y'),
        // Greek: Q and W positions carry `;` and final sigma.
        ("el-GR", 0x10, false, ';'),
        ("el-GR", 0x11, false, 'ς'),
        ("el-GR", 0x1E, false, 'α'),
        // Hebrew is unicase — the letter positions are RTL glyphs.
        ("he-IL", 0x1E, false, 'ש'),
        ("he-IL", 0x14, false, 'א'),
        // Turkish's dotless/dotted i pair, the one that bites.
        ("tr-TR", 0x17, false, 'ı'),
        ("tr-TR", 0x17, true, 'I'),
        ("tr-TR", 0x28, false, 'i'),
        ("tr-TR", 0x28, true, 'İ'),
        // Bulgarian BDS is frequency-ordered, not phonetic: the `Q`
        // position is a comma and `ъ` sits on `C`.
        ("bg-BG", 0x10, false, ','),
        ("bg-BG", 0x2E, false, 'ъ'),
        ("bg-BG", 0x56, false, 'ѝ'),
        ("it-IT", 0x27, false, 'ò'),
        ("it-IT", 0x28, false, 'à'),
        // Portuguese: ç on the `;` position in both orthographies,
        // and the dead keys surfaced as spacing forms.
        ("pt-PT", 0x27, false, 'ç'),
        ("pt-PT", 0x1B, false, '´'),
        ("pt-BR", 0x27, false, 'ç'),
        ("pt-BR", 0x28, false, '~'),
    ];
    for (id, sc, shift, expected) in cases {
        let mapping = db.get(&LayoutId::from(id)).unwrap_or_else(|| {
            panic!("layout `{id}` not loaded");
        });
        let got = mapping.translate_key(WordKey {
            scancode: sc,
            shift,
            timestamp_ms: 0,
        });
        assert_eq!(
            got,
            Some(expected),
            "layout {id} sc=0x{sc:X} shift={shift}: expected `{expected}` got {got:?}"
        );
    }
}

#[test]
fn wordlists_loaded_with_layouts() {
    let db = LayoutDb::load_embedded();
    let en = db.get(&LayoutId::from("en-US")).expect("en-US");
    let uk = db.get(&LayoutId::from("uk-UA")).expect("uk-UA");
    let en_dict = en.dictionary.as_ref().expect("en dictionary");
    let uk_dict = uk.dictionary.as_ref().expect("uk dictionary");
    for w in ["the", "hello", "a", "i", "function", "world", "code"] {
        assert!(en_dict.contains(w), "en dict missing `{w}`");
    }
    for w in [
        "що", "мені", "цим", "а", "і", "у", "о", "є", "я", "з", "в", "й",
    ] {
        assert!(uk_dict.contains(w), "uk dict missing `{w}`");
    }
    for w in ["слово", "привіт", "робити", "знати"] {
        assert!(uk_dict.contains(w), "uk dict missing `{w}`");
    }
}

/// Every bundled language must recognise ordinary words of its own,
/// **including ones that carry the script's diacritics**.
///
/// That second half is the point. Polish and Greek shipped through a
/// `.dic` decoded with the wrong codepage: the ASCII-only words were
/// all present and correct, so any spot-check that stuck to `jest` or
/// `kai` passed, while `słowo` sat in the FST as `s³owo`. Each row
/// below therefore mixes plain words with accented / non-Latin ones.
#[test]
fn every_bundled_dictionary_knows_its_own_words() {
    let db = LayoutDb::load_embedded();
    let cases: &[(&str, &[&str])] = &[
        ("pl-PL", &["jest", "bardzo", "dzień", "słowo", "książka"]),
        ("cs-CZ", &["ahoj", "slovo", "dobrý", "protože", "děkuji"]),
        ("el-GR", &["λέξη", "είναι", "καλημέρα", "ευχαριστώ"]),
        ("he-IL", &["שלום", "מילה", "ספר", "תודה"]),
        (
            "tr-TR",
            &["kitap", "merhaba", "kelime", "güzel", "teşekkür"],
        ),
        ("bg-BG", &["дума", "книга", "здравей", "благодаря"]),
        ("it-IT", &["parola", "grazie", "perché", "buongiorno"]),
        ("pt-PT", &["palavra", "obrigado", "coração", "português"]),
        ("pt-BR", &["palavra", "obrigado", "você", "português"]),
    ];
    for (id, words) in cases {
        let layout = db
            .get(&LayoutId::from(*id))
            .unwrap_or_else(|| panic!("layout `{id}` not loaded"));
        let dict = layout
            .dictionary
            .as_ref()
            .unwrap_or_else(|| panic!("`{id}` has no dictionary — did the fetch fail for it?"));
        for w in *words {
            assert!(
                dict.contains(w),
                "{id} dictionary missing `{w}` — if the plain-ASCII words in \
                 this row pass and only the accented ones fail, suspect the \
                 codepage the .dic was decoded with"
            );
        }
    }
}

#[test]
fn round_trip_hello_through_uk() {
    let db = LayoutDb::load_embedded();
    let en = db.get(&LayoutId::from("en-US")).expect("en-US");
    let uk = db.get(&LayoutId::from("uk-UA")).expect("uk-UA");
    let buf = vec![
        WordKey {
            scancode: 0x23,
            shift: false,
            timestamp_ms: 0,
        },
        WordKey {
            scancode: 0x12,
            shift: false,
            timestamp_ms: 0,
        },
        WordKey {
            scancode: 0x26,
            shift: false,
            timestamp_ms: 0,
        },
        WordKey {
            scancode: 0x26,
            shift: false,
            timestamp_ms: 0,
        },
        WordKey {
            scancode: 0x18,
            shift: false,
            timestamp_ms: 0,
        },
    ];
    assert_eq!(en.translate_buffer(&buf), "hello");
    assert_eq!(uk.translate_buffer(&buf), "руддщ");
}

#[test]
fn round_trip_pryvit_through_en() {
    let db = LayoutDb::load_embedded();
    let en = db.get(&LayoutId::from("en-US")).expect("en-US");
    let uk = db.get(&LayoutId::from("uk-UA")).expect("uk-UA");
    let buf = vec![
        WordKey {
            scancode: 0x22,
            shift: false,
            timestamp_ms: 0,
        },
        WordKey {
            scancode: 0x23,
            shift: false,
            timestamp_ms: 0,
        },
        WordKey {
            scancode: 0x30,
            shift: false,
            timestamp_ms: 0,
        },
        WordKey {
            scancode: 0x20,
            shift: false,
            timestamp_ms: 0,
        },
        WordKey {
            scancode: 0x1F,
            shift: false,
            timestamp_ms: 0,
        },
        WordKey {
            scancode: 0x31,
            shift: false,
            timestamp_ms: 0,
        },
    ];
    assert_eq!(uk.translate_buffer(&buf), "привіт");
    let en_text = en.translate_buffer(&buf);
    assert!(en_text.is_ascii());
}

#[test]
fn shift_picks_uppercase() {
    let db = LayoutDb::load_embedded();
    let en = db.get(&LayoutId::from("en-US")).expect("en-US");
    let buf = vec![WordKey {
        scancode: 0x23,
        shift: true,
        timestamp_ms: 0,
    }];
    assert_eq!(en.translate_buffer(&buf), "H");
}

// ─── User overlay loading (runtime-extensible) ───────────────────

struct TmpDir(PathBuf);

impl TmpDir {
    fn new(label: &str) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "poltertype-test-{label}-{}-{now}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("mkdir tmp");
        Self(path)
    }

    fn write(&self, name: &str, body: &str) {
        std::fs::write(self.0.join(name), body).expect("write tmp file");
    }
}

impl Drop for TmpDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn user_overlay_picks_up_extras_file() {
    let tmp = TmpDir::new("extras");
    tmp.write("uk_ua.txt", "# user adds\nфайв\n");
    tmp.write("uk_ua-extras.txt", "# more\nекстраслово\n");

    let db = LayoutDb::load_embedded_with_user_overlay(Some(&tmp.0));
    let dict = db
        .get(&LayoutId::from("uk-UA"))
        .and_then(|l| l.dictionary.as_ref())
        .expect("uk dict");

    assert!(dict.contains("файв"), "<stem>.txt entry should be in dict");
    assert!(
        dict.contains("екстраслово"),
        "<stem>-extras.txt entry should be in dict"
    );
}

#[test]
fn user_overlay_normalizes_hyphens_and_apostrophes() {
    // Regression: the wordlists tab lets users add hand-picked
    // tokens to the per-layout dictionary, but hyphenated /
    // apostrophe-bearing entries used to be stored verbatim while
    // the lookup path canonicalised the typed token via
    // `letters_only_lower`. End-result: `v-strel-zbook` in the
    // extras file never matched the buffer `v-strel-zbook`
    // (lookup key `vstrelzbook`) and the engine kept switching
    // it. Lock the canonicalisation in: the entry as written and
    // the canonical key must both resolve to a Keep.
    let tmp = TmpDir::new("normalize-hyphen");
    tmp.write("en_us-extras.txt", "v-strel-zbook\n");
    tmp.write("uk_ua-extras.txt", "ім'я\n");

    let db = LayoutDb::load_embedded_with_user_overlay(Some(&tmp.0));

    let en = db
        .get(&LayoutId::from("en-US"))
        .and_then(|l| l.dictionary.as_ref())
        .expect("en dict");
    assert!(
        en.contains("vstrelzbook"),
        "hyphenated extras entry must be looked up by its \
         letters-only canonical key"
    );

    let uk = db
        .get(&LayoutId::from("uk-UA"))
        .and_then(|l| l.dictionary.as_ref())
        .expect("uk dict");
    assert!(
        uk.contains("імя"),
        "apostrophe-bearing extras entry must be looked up by \
         its letters-only canonical key"
    );
}

/// Regression: 2-letter acronyms in `<stem>-extras.txt` (`ai`,
/// `ml`, `ui`, `ux`, `db`, …) used to land **only** in the
/// embedded FST, but the runtime short-token lookup deliberately
/// skips the FST (the bulk dict ships short-noise like `ws` /
/// `ax` / `oe`). Result: typing `AI` while in uk-UA produced
/// `ФШ` and neither detector had any signal to switch — the user
/// was stuck. `build.rs` now mirrors the ≤2-letter slice of
/// extras into the dist `<stem>-stop.txt`, so the short regime
/// sees them.
#[test]
fn short_extras_are_visible_to_short_token_lookup() {
    let db = LayoutDb::load_embedded();
    let en = db
        .get(&LayoutId::from("en-US"))
        .and_then(|l| l.dictionary.as_ref())
        .expect("en dict");

    // Exhaustive sample — every one of these is a 2-letter entry
    // in `data/wordlists/en_us-extras.txt` that a developer might
    // type as a standalone token. If the build pipeline ever
    // regresses to leaving them FST-only, this test fails loud.
    for word in ["ai", "ml", "ui", "ux", "db", "qa", "cd", "ci", "md"] {
        assert!(
            en.contains_short(word),
            "en-US `{word}` should be visible to the short-token \
             lookup (mirrored from en_us-extras.txt by build.rs)"
        );
    }

    // Sanity: the FST-only `dwyl/english-words` short noise
    // (`ws`, `ax`, `oe`) must NOT leak in. If it does the fix
    // overshot and would block legitimate Cyrillic switches.
    for noise in ["ws", "ax", "oe"] {
        assert!(
            !en.contains_short(noise),
            "en-US `{noise}` is bulk-dict short noise — must NOT \
             be in short_stop_words"
        );
    }
}

/// End-to-end regression for the weak-list pipeline: the
/// bundled uk-UA dict ships with `туче` flagged weak (vocative
/// of `туча`, "O cloud!"), so the dictionary detector must
/// switch to en-US `next` when both are dict hits.
#[test]
fn bundled_weak_list_marks_tuche() {
    let db = LayoutDb::load_embedded();
    let uk = db
        .get(&LayoutId::from("uk-UA"))
        .and_then(|l| l.dictionary.as_ref())
        .expect("uk dict");
    // Sanity that `туче` IS in the bundled FST — the test would
    // pass vacuously if Hunspell ever stops emitting it.
    assert!(uk.contains("туче"), "`туче` must be in the bundled uk FST");
    assert!(
        uk.is_weak("туче"),
        "`туче` must be on the bundled uk-UA weak list — \
         see data/wordlists/uk_ua-weak.txt"
    );
}

#[test]
fn user_weak_file_extends_weak_list() {
    let tmp = TmpDir::new("weak");
    tmp.write("uk_ua-weak.txt", "# user adds\nтестслабке\n");

    let db = LayoutDb::load_embedded_with_user_overlay(Some(&tmp.0));
    let dict = db
        .get(&LayoutId::from("uk-UA"))
        .and_then(|l| l.dictionary.as_ref())
        .expect("uk dict");
    assert!(
        dict.is_weak("тестслабке"),
        "user-side -weak.txt should extend the weak list"
    );
}

#[test]
fn user_short_stop_file_extends_stop_list() {
    let tmp = TmpDir::new("stop");
    tmp.write("uk_ua-stop.txt", "хм\n");

    let db = LayoutDb::load_embedded_with_user_overlay(Some(&tmp.0));
    let dict = db
        .get(&LayoutId::from("uk-UA"))
        .and_then(|l| l.dictionary.as_ref())
        .expect("uk dict");

    assert!(
        dict.contains_short("хм"),
        "user-side -stop.txt should extend short stop list"
    );
}

#[test]
fn missing_user_files_do_not_break_loading() {
    let tmp = TmpDir::new("empty");
    let db = LayoutDb::load_embedded_with_user_overlay(Some(&tmp.0));
    assert!(db.get(&LayoutId::from("en-US")).is_some());
    assert!(db.get(&LayoutId::from("uk-UA")).is_some());
}

fn minimal_layout_toml(id: &str, name: &str, script: &str) -> String {
    format!(
        r#"
id     = "{id}"
name   = "{name}"
script = "{script}"

[keys]
0x10 = {{ plain = "x", shift = "X" }}
0x11 = {{ plain = "y", shift = "Y" }}
"#,
    )
}

#[test]
fn user_layout_dir_adds_extra_layout() {
    let layout_tmp = TmpDir::new("user-layouts-add");
    std::fs::write(
        layout_tmp.0.join("kk_kz.toml"),
        minimal_layout_toml("kk-KZ", "Қазақ", "Cyrillic"),
    )
    .expect("write user layout");

    let db = LayoutDb::load_with_user_layouts(Some(&layout_tmp.0), None);
    assert!(
        db.get(&LayoutId::from("kk-KZ")).is_some(),
        "user-side TOML at <dir>/kk_kz.toml should load as kk-KZ"
    );
    assert!(db.get(&LayoutId::from("en-US")).is_some());
}

#[test]
fn user_layout_overrides_embedded_with_same_id() {
    let layout_tmp = TmpDir::new("user-layouts-override");
    std::fs::write(
        layout_tmp.0.join("en_us.toml"),
        minimal_layout_toml("en-US", "USER-OVERRIDE-EN", "Latin"),
    )
    .expect("write user layout");

    let db = LayoutDb::load_with_user_layouts(Some(&layout_tmp.0), None);
    let en = db.get(&LayoutId::from("en-US")).expect("en-US present");
    assert_eq!(
        en.name, "USER-OVERRIDE-EN",
        "user TOML should win over embedded layout"
    );
}

#[test]
fn malformed_user_layout_is_skipped() {
    let layout_tmp = TmpDir::new("user-layouts-malformed");
    std::fs::write(
        layout_tmp.0.join("bad.toml"),
        "this is not valid TOML at all <<<>>>",
    )
    .expect("write bad layout");

    let db = LayoutDb::load_with_user_layouts(Some(&layout_tmp.0), None);
    assert!(db.get(&LayoutId::from("en-US")).is_some());
    assert!(db.get(&LayoutId::from("bad")).is_none());
}

#[test]
fn user_layout_picks_up_matching_wordlist() {
    let layout_tmp = TmpDir::new("user-layouts-dict-l");
    let overlay_tmp = TmpDir::new("user-layouts-dict-w");
    std::fs::write(
        layout_tmp.0.join("kk_kz.toml"),
        minimal_layout_toml("kk-KZ", "Қазақ", "Cyrillic"),
    )
    .expect("write user layout");
    std::fs::write(overlay_tmp.0.join("kk_kz.txt"), "тілқолданбасы\n")
        .expect("write user wordlist");

    let db = LayoutDb::load_with_user_layouts(Some(&layout_tmp.0), Some(&overlay_tmp.0));
    let dict = db
        .get(&LayoutId::from("kk-KZ"))
        .and_then(|l| l.dictionary.as_ref())
        .expect("kk-KZ dictionary built from overlay");
    assert!(dict.contains("тілқолданбасы"));
}

// ─── Plug-in pack loader ───────────────────────────────────────

/// Minimal but real FST blob for "must compile, must read back"
/// tests. We don't need a populated dictionary to verify the
/// loader picks up the file — we just need the bytes to parse as
/// a valid `FstSet`.
fn empty_fst_bytes() -> Vec<u8> {
    let builder = fst::SetBuilder::memory();
    builder.into_inner().expect("empty FST builder")
}

/// Happy path: drop a complete plug-in tree under
/// `<data_dir>/plugins/<pack>/` and verify the contained layout
/// shows up in the loaded `LayoutDb` with its dictionary attached.
#[test]
fn plugin_pack_layout_loads() {
    let data_dir = TmpDir::new("plugin-happy");
    let pack_dir = data_dir.0.join("plugins").join("test-pack");
    std::fs::create_dir_all(pack_dir.join("layout-mappings")).unwrap();
    std::fs::create_dir_all(pack_dir.join("wordlists")).unwrap();

    std::fs::write(
        pack_dir.join("manifest.toml"),
        r#"
id = "test-pack"
name = "Test pack"
version = "0.0.1"
supported_layouts = ["kk-KZ"]
"#,
    )
    .unwrap();
    std::fs::write(
        pack_dir.join("layout-mappings").join("kk_kz.toml"),
        minimal_layout_toml("kk-KZ", "Қазақ", "Cyrillic"),
    )
    .unwrap();
    std::fs::write(
        pack_dir.join("wordlists").join("kk_kz.fst"),
        empty_fst_bytes(),
    )
    .unwrap();

    let db = LayoutDb::load(LoadOptions {
        data_dir: Some(&data_dir.0),
        ..Default::default()
    })
    .expect("load plugin pack");

    let layout = db
        .get(&LayoutId::from("kk-KZ"))
        .expect("plug-in's kk-KZ layout should be loaded");
    // Dictionary attached because we shipped an FST in the pack.
    assert!(
        layout.dictionary.is_some(),
        "plug-in with shipped FST should have a dictionary"
    );
}

/// A plug-in directory without a `manifest.toml` is skipped, but
/// the rest of the load proceeds. Without this guard, a stray
/// folder under `plugins/` (e.g. `.git/`, `__MACOSX/` from an
/// extracted zip) could derail every other pack.
#[test]
fn plugin_missing_manifest_skipped_gracefully() {
    let data_dir = TmpDir::new("plugin-no-manifest");
    let pack_dir = data_dir.0.join("plugins").join("broken-pack");
    std::fs::create_dir_all(pack_dir.join("layout-mappings")).unwrap();
    // No manifest.toml. There's even a TOML in layout-mappings
    // that *would* parse — but since the pack lacks a manifest
    // we should refuse to load anything from it.
    std::fs::write(
        pack_dir.join("layout-mappings").join("kk_kz.toml"),
        minimal_layout_toml("kk-KZ", "Қазақ", "Cyrillic"),
    )
    .unwrap();

    let db = LayoutDb::load(LoadOptions {
        data_dir: Some(&data_dir.0),
        ..Default::default()
    })
    .expect("load with broken pack");
    assert!(
        db.get(&LayoutId::from("kk-KZ")).is_none(),
        "layout from a manifest-less pack must not be loaded"
    );
}

/// Plug-in's TOML with an unparseable manifest is skipped. The
/// pack is logged + ignored; other packs in the same plug-ins
/// directory keep loading.
#[test]
fn plugin_invalid_manifest_skipped() {
    let data_dir = TmpDir::new("plugin-bad-manifest");
    let bad_pack = data_dir.0.join("plugins").join("bad-pack");
    std::fs::create_dir_all(bad_pack.join("layout-mappings")).unwrap();
    std::fs::write(bad_pack.join("manifest.toml"), "not = valid = toml === ").unwrap();
    std::fs::write(
        bad_pack.join("layout-mappings").join("kk_kz.toml"),
        minimal_layout_toml("kk-KZ", "Қазақ", "Cyrillic"),
    )
    .unwrap();

    // A second pack alongside, this one well-formed — must still
    // load to prove the bad pack didn't poison the whole load.
    let good_pack = data_dir.0.join("plugins").join("good-pack");
    std::fs::create_dir_all(good_pack.join("layout-mappings")).unwrap();
    std::fs::create_dir_all(good_pack.join("wordlists")).unwrap();
    std::fs::write(
        good_pack.join("manifest.toml"),
        r#"id="good-pack"
name="Good"
version="0.1.0""#,
    )
    .unwrap();
    std::fs::write(
        good_pack.join("layout-mappings").join("kk_kz.toml"),
        minimal_layout_toml("kk-KZ", "Қазақ from good pack", "Cyrillic"),
    )
    .unwrap();
    std::fs::write(
        good_pack.join("wordlists").join("kk_kz.fst"),
        empty_fst_bytes(),
    )
    .unwrap();

    let db = LayoutDb::load(LoadOptions {
        data_dir: Some(&data_dir.0),
        ..Default::default()
    })
    .expect("load with mixed packs");
    let layout = db
        .get(&LayoutId::from("kk-KZ"))
        .expect("good pack should still load even though bad pack lives next to it");
    assert_eq!(
        layout.name, "Қазақ from good pack",
        "the good pack's layout should win — not the bad pack's TOML"
    );
}

/// User overlay TOML overrides a plug-in TOML with the same id.
/// This is the documented precedence chain
/// `bundled ← plug-ins ← user-overlay`. A user dropping a
/// matching TOML under `<config-dir>/poltertype/layouts/` should
/// always win.
#[test]
fn user_overlay_overrides_plugin() {
    let data_dir = TmpDir::new("plugin-vs-user-data");
    let user_dir = TmpDir::new("plugin-vs-user-layouts");

    let pack = data_dir.0.join("plugins").join("p");
    std::fs::create_dir_all(pack.join("layout-mappings")).unwrap();
    std::fs::create_dir_all(pack.join("wordlists")).unwrap();
    std::fs::write(
        pack.join("manifest.toml"),
        r#"id="p"
name="Pack"
version="0.0.1""#,
    )
    .unwrap();
    std::fs::write(
        pack.join("layout-mappings").join("kk_kz.toml"),
        minimal_layout_toml("kk-KZ", "FROM-PACK", "Cyrillic"),
    )
    .unwrap();
    std::fs::write(pack.join("wordlists").join("kk_kz.fst"), empty_fst_bytes()).unwrap();

    // The user's own copy of `kk-KZ`.
    std::fs::write(
        user_dir.0.join("kk_kz.toml"),
        minimal_layout_toml("kk-KZ", "FROM-USER", "Cyrillic"),
    )
    .unwrap();

    let db = LayoutDb::load(LoadOptions {
        data_dir: Some(&data_dir.0),
        user_layout_dir: Some(&user_dir.0),
        ..Default::default()
    })
    .expect("load with plugin + user overlap");
    let layout = db.get(&LayoutId::from("kk-KZ")).expect("kk-KZ");
    assert_eq!(
        layout.name, "FROM-USER",
        "user overlay must win over plug-in for the same id"
    );
}

#[test]
fn user_layout_without_wordlist_still_loads() {
    let layout_tmp = TmpDir::new("user-layouts-nodict-l");
    let overlay_tmp = TmpDir::new("user-layouts-nodict-w");
    std::fs::write(
        layout_tmp.0.join("kk_kz.toml"),
        minimal_layout_toml("kk-KZ", "Қазақ", "Cyrillic"),
    )
    .expect("write user layout");

    let db = LayoutDb::load_with_user_layouts(Some(&layout_tmp.0), Some(&overlay_tmp.0));
    let layout = db.get(&LayoutId::from("kk-KZ")).expect("kk-KZ loaded");
    assert!(
        layout.dictionary.is_none(),
        "no overlay file → no dictionary attached"
    );
}

#[test]
fn overlay_is_freshly_read_on_each_build() {
    let tmp = TmpDir::new("reload");
    let first_token = "zxqzxqfirst";
    let second_token = "qwrqwrsecond";

    tmp.write("uk_ua.txt", &format!("{first_token}\n"));
    let first = LayoutDb::load_embedded_with_user_overlay(Some(&tmp.0));
    let first_dict = first
        .get(&LayoutId::from("uk-UA"))
        .and_then(|l| l.dictionary.as_ref())
        .expect("uk dict #1");
    assert!(first_dict.contains(first_token));
    assert!(!first_dict.contains(second_token));

    tmp.write("uk_ua.txt", &format!("{second_token}\n"));
    let second = LayoutDb::load_embedded_with_user_overlay(Some(&tmp.0));
    let second_dict = second
        .get(&LayoutId::from("uk-UA"))
        .and_then(|l| l.dictionary.as_ref())
        .expect("uk dict #2");
    assert!(second_dict.contains(second_token));
    assert!(
        !second_dict.contains(first_token),
        "old overlay must not leak into the fresh load"
    );
}

// ─── OS keymaps: a language is not a keyboard (issue #20) ─────────

/// A keyboard the OS could plausibly describe: `n` keys from scancode
/// `0x10` up, each producing a distinct character from `base` on, so a
/// test can tell an adopted table from a bundled one at a glance.
fn os_keymap(id: &str, variant: &str, n: u32, base: char) -> OsKeymap {
    let keys = (0..n)
        .map(|i| {
            let ch = char::from_u32(base as u32 + i).expect("test alphabet stays in range");
            (0x10 + i, ch, None)
        })
        .collect();
    OsKeymap {
        id: LayoutId::from(id),
        variant: variant.to_owned(),
        keys,
    }
}

fn plain(scancode: u32) -> WordKey {
    WordKey {
        scancode,
        shift: false,
        timestamp_ms: 0,
    }
}

/// The whole point of #20: when the OS describes the keyboard the user
/// actually has, that description wins over the one we guessed.
#[test]
fn an_os_keymap_replaces_the_bundled_key_table() {
    let want = [LayoutId::from("bg-BG")];
    let maps = [os_keymap("bg-BG", "00040402", 40, 'а')];
    let db = LayoutDb::load(LoadOptions {
        active_filter: Some(&want),
        os_keymaps: Some(&maps),
        ..Default::default()
    })
    .expect("load with OS keymaps");

    let bg = db.get(&LayoutId::from("bg-BG")).expect("bg-BG present");
    assert_eq!(
        bg.keys.len(),
        40,
        "the OS table describes the whole keyboard, so it replaces rather than merges"
    );
    assert_eq!(
        bg.translate_key(plain(0x10)),
        Some('а'),
        "0x10 should come from the OS ('а'), not from bg_bg.toml (',')"
    );

    // Identity is per-*language* and has to survive: the name, the
    // script and the dictionary all describe Bulgarian, not a
    // particular Bulgarian keyboard.
    assert_eq!(bg.name, "Български");
    assert!(
        bg.dictionary.is_some(),
        "adopting a keymap must not cost the layout its dictionary"
    );
}

/// A key table without a dictionary detects nothing, so a language we
/// ship no mapping for is left alone rather than half-invented.
#[test]
fn an_os_keymap_for_a_language_we_do_not_ship_is_ignored() {
    let maps = [os_keymap("kk-KZ", "0000043f", 40, 'а')];
    let db = LayoutDb::load(LoadOptions {
        os_keymaps: Some(&maps),
        ..Default::default()
    })
    .expect("load with an unknown-language keymap");

    assert!(db.get(&LayoutId::from("kk-KZ")).is_none());
    assert_eq!(db.len(), BUNDLED_LAYOUT_STEMS.len());
}

/// The floor under a query that went wrong. A mapping that is right
/// for the wrong variant still beats one missing half its alphabet.
#[test]
fn a_sparse_os_keymap_is_refused() {
    let want = [LayoutId::from("bg-BG")];
    let sparse = MIN_OS_KEYMAP_KEYS as u32 - 1;
    let maps = [os_keymap("bg-BG", "00040402", sparse, 'а')];
    let db = LayoutDb::load(LoadOptions {
        active_filter: Some(&want),
        os_keymaps: Some(&maps),
        ..Default::default()
    })
    .expect("load with a sparse OS keymap");

    let bg = db.get(&LayoutId::from("bg-BG")).expect("bg-BG present");
    assert_eq!(
        bg.translate_key(plain(0x10)),
        Some(','),
        "bg_bg.toml should still be in charge"
    );
}

/// Two keyboards for one language collapse to one `LayoutId` and only
/// one table can be held. The backend puts the keyboard currently in
/// effect first; the loader keeps that one.
#[test]
fn only_the_first_keyboard_per_language_is_adopted() {
    let want = [LayoutId::from("bg-BG")];
    let maps = [
        os_keymap("bg-BG", "00030402", 40, 'а'),
        os_keymap("bg-BG", "00040402", 40, 'ѐ'),
    ];
    let db = LayoutDb::load(LoadOptions {
        active_filter: Some(&want),
        os_keymaps: Some(&maps),
        ..Default::default()
    })
    .expect("load with two keyboards for one language");

    let bg = db.get(&LayoutId::from("bg-BG")).expect("bg-BG present");
    assert_eq!(bg.translate_key(plain(0x10)), Some('а'));
}

/// The escape hatch. If this mechanism ever reads a keyboard wrong,
/// a user TOML is how someone takes back control — so it has to
/// outrank the OS, not the other way round.
#[test]
fn a_user_toml_still_outranks_an_os_keymap() {
    let layout_tmp = TmpDir::new("os-keymap-user-wins");
    layout_tmp.write(
        "en_us.toml",
        &minimal_layout_toml("en-US", "USER-OVERRIDE-EN", "Latin"),
    );

    let want = [LayoutId::from("en-US")];
    let maps = [os_keymap("en-US", "00000409", 40, 'а')];
    let db = LayoutDb::load(LoadOptions {
        active_filter: Some(&want),
        user_layout_dir: Some(&layout_tmp.0),
        os_keymaps: Some(&maps),
        ..Default::default()
    })
    .expect("load with a user TOML and an OS keymap");

    let en = db.get(&LayoutId::from("en-US")).expect("en-US present");
    assert_eq!(en.name, "USER-OVERRIDE-EN");
    assert_eq!(
        en.translate_key(plain(0x10)),
        Some('x'),
        "the user's own table must survive the OS overlay intact"
    );
}

/// `key_for_char` iterates a `HashMap`, and an OS-derived table really
/// does put one character on two keys — en-US carries `\` on both
/// `0x2B` and the extra ISO key `0x56`. Without a tie-break the
/// suggestion-accept path would pick a different key run to run.
#[test]
fn key_for_char_breaks_ties_on_the_lowest_scancode() {
    let toml = r#"
id     = "zz-ZZ"
name   = "Ambiguous"
script = "Latin"

[keys]
0x2B = { plain = "\\", shift = "|" }
0x56 = { plain = "\\", shift = "|" }
"#;
    let mapping = LayoutMapping::from_toml_str(toml).expect("parse");
    for _ in 0..32 {
        assert_eq!(mapping.key_for_char('\\'), Some((0x2B, false)));
        assert_eq!(mapping.key_for_char('|'), Some((0x2B, true)));
    }
}
