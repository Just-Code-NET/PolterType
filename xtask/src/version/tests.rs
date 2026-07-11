use super::*;

/// The parser must accept every shape of version string we've
/// ever shipped (and a couple of likely-future shapes) — and
/// reject the kinds of typos a hand-edited config could have.
#[test]
fn parse_accepts_known_shapes() {
    for ok in [
        "0.1.0",
        "0.1.0-alpha.0",
        "0.1.0-alpha.5",
        "0.1.0-beta.6",
        "0.1.0-rc.1",
        "1.2.3",
        "10.20.30",
    ] {
        assert!(parse(ok).is_ok(), "`{ok}` should parse");
    }
}

#[test]
fn parse_rejects_garbage() {
    for bad in [
        "",
        "1",
        "1.2",
        "1.2.3.4",          // four segments
        "1.2.x",            // non-numeric patch
        "1.2.3-",           // empty pre-release
        "1.2.3-beta",       // pre-release missing counter
        "1.2.3-beta.x",     // non-numeric counter
        "1.2.3-rc.1.extra", // multi-segment pre-release
        "1.2.3-be ta.1",    // whitespace in word
        "1.2.3-rc1.0",      // word contains digits (we forbid this — keep it simple)
    ] {
        assert!(parse(bad).is_err(), "`{bad}` should NOT parse");
    }
}

/// Display round-trip: parse, format, parse again — must yield
/// the same struct. Catches subtle drift in the formatter.
#[test]
fn parse_format_round_trip() {
    for v in [
        "0.1.0",
        "0.1.0-alpha.0",
        "0.1.0-beta.6",
        "1.2.3-rc.99",
        "10.20.30",
    ] {
        let parsed = parse(v).expect("parse");
        assert_eq!(parsed.to_string(), v);
    }
}

/// `bump` rules pinned in tests so we can never accidentally
/// change the auto-bump semantics without noticing.
#[test]
fn bump_pre_release_increments_counter() {
    assert_eq!(bump("0.1.0-beta.5").unwrap(), "0.1.0-beta.6");
    assert_eq!(bump("0.1.0-alpha.0").unwrap(), "0.1.0-alpha.1");
    assert_eq!(bump("1.2.3-rc.99").unwrap(), "1.2.3-rc.100");
}

#[test]
fn bump_stable_increments_patch() {
    assert_eq!(bump("0.1.0").unwrap(), "0.1.1");
    assert_eq!(bump("1.2.3").unwrap(), "1.2.4");
    assert_eq!(bump("0.1.99").unwrap(), "0.1.100");
}

/// Cargo.toml line surgery — must replace the workspace
/// version while leaving dep pins (`version = "1.0"`) alone.
/// This is the regression we care most about: a global string
/// replace would silently demote every internal crate version
/// line in a workspace.dependencies block.
#[test]
fn replace_version_line_only_touches_first_occurrence() {
    let body = r#"
[workspace.package]
version       = "0.1.0-beta.6"
edition       = "2024"

[workspace.dependencies]
some-crate = { version = "0.1.0-beta.6" }
"#;
    let out = replace_version_line(body, "0.1.0-beta.6", "0.1.0-beta.7").unwrap();
    // First occurrence (the workspace.package one) is bumped.
    assert!(out.contains("version       = \"0.1.0-beta.7\""));
    // Second occurrence (the dep entry) is NOT touched.
    assert!(out.contains("some-crate = { version = \"0.1.0-beta.6\" }"));
}

/// `read_version` must locate the workspace version even when
/// other `version` keys appear in the file (in deps, in
/// comments, etc.). We don't care about the exact ordering of
/// fields, only that the first `version = "..."` line wins —
/// which is also what `replace_version_line` relies on.
#[test]
fn read_version_finds_workspace_package_version() {
    let body = r#"
# version of the schema, unrelated.
[workspace.package]
version       = "0.1.0-beta.6"
edition       = "2024"
"#;
    assert_eq!(read_version(body).unwrap(), "0.1.0-beta.6");
}

/// `read_version` produces a clear error when the file doesn't
/// have the expected shape — the script is more useful when it
/// fails loudly than when it pretends to succeed.
#[test]
fn read_version_errors_on_missing_field() {
    let body = "[package]\nname = \"nope\"\n";
    assert!(read_version(body).is_err());
}
