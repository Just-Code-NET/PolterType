use super::*;

/// The regression that made this module worth testing: `pl_PL` and
/// `el_GR` declare non-Latin-1 codepages, and decoding their `.dic`
/// as Latin-1 produced plausible-looking mojibake — `słowo` came out
/// as `s³owo`, so it neither matched a lookup nor tripped any check.
#[test]
fn latin2_high_bytes_decode_to_polish_letters() {
    // 0xB3 0xF1 0xBF 0xB9 is `łńżš` in ISO-8859-2 and `³ñ¿¹` in
    // Latin-1.
    let decoded = decode_high(&[0xB3, 0xF1, 0xBF, 0xB9], &LATIN2_HIGH);
    assert_eq!(decoded, "łńżš");
}

#[test]
fn greek_high_bytes_decode_to_greek_letters() {
    // `αλφά` — the same bytes read as Latin-1 give `áëöÜ`.
    let decoded = decode_high(&[0xE1, 0xEB, 0xF6, 0xDC], &GREEK_HIGH);
    assert_eq!(decoded, "αλφά");
}

#[test]
fn codepages_agree_with_ascii_below_a0() {
    for table in [&LATIN2_HIGH, &GREEK_HIGH] {
        let ascii: Vec<u8> = (0x20u8..0x7F).collect();
        let decoded = decode_high(&ascii, table);
        assert!(
            decoded.chars().eq(ascii.iter().map(|&b| b as char)),
            "bytes below 0xA0 must pass through unchanged"
        );
    }
}

#[test]
fn set_directive_selects_the_codepage() {
    let cases = [
        ("SET UTF-8\n", Encoding::Utf8),
        ("SET ISO8859-1\n", Encoding::Latin1),
        ("SET ISO-8859-1\n", Encoding::Latin1),
        ("SET WINDOWS-1252\n", Encoding::Latin1),
        ("SET ISO8859-2\n", Encoding::Latin2),
        ("SET ISO8859-7\n", Encoding::Greek),
        // Real files lead with comments — el_GR does exactly this.
        (
            "# greek affix file\n#\nSET ISO8859-7\nTRY abc\n",
            Encoding::Greek,
        ),
    ];
    for (text, want) in cases {
        let got = encoding_of_aff(text.as_bytes()).expect("declared encoding should be recognised");
        assert_eq!(got, want, "for `{}`", text.escape_debug());
    }
}

/// pt_BR's `.aff` opens with a UTF-8 BOM. Left on, it glues itself to
/// the `SET` keyword on line 1 and the directive reads as absent.
#[test]
fn leading_bom_does_not_hide_the_set_line() {
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(b"SET UTF-8\nFLAG UTF-8\n");
    assert_eq!(
        encoding_of_aff(&bytes).expect("BOM must be skipped"),
        Encoding::Utf8
    );
}

/// Both of these used to silently mean "decode as Latin-1", which is
/// the behaviour that shipped broken dictionaries. They must fail.
#[test]
fn unknown_and_missing_encodings_are_errors() {
    let err = encoding_of_aff(b"SET KOI8-R\n").expect_err("unknown codepage must fail");
    assert!(err.to_string().contains("KOI8-R"), "names it: {err}");

    let err = encoding_of_aff(b"TRY abc\nSFX A Y 0\n").expect_err("missing SET must fail");
    assert!(err.to_string().contains("SET"), "explains itself: {err}");
}
