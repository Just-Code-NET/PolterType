use super::*;

fn cmd(program: &str, args: &[&str], insert: bool) -> ShellCommand {
    ShellCommand {
        program: program.into(),
        args: args.iter().map(|s| (*s).to_string()).collect(),
        insert_output: insert,
    }
}

// ── the gate ─────────────────────────────────────────────────────────

/// The default. A config full of `run_shell` entries on a machine
/// that never enabled them must run nothing.
#[test]
fn nothing_runs_while_the_setting_is_off() {
    assert_eq!(
        check(&cmd("echo", &["hi"], false), false),
        Err(ShellRefusal::NotEnabled)
    );
}

#[test]
fn the_refusal_says_which_setting_to_flip() {
    let msg = ShellRefusal::NotEnabled.to_string();
    assert!(msg.contains("allow_run_shell"), "{msg}");
}

#[test]
fn an_empty_program_is_refused_even_when_enabled() {
    assert_eq!(
        check(&cmd("   ", &[], false), true),
        Err(ShellRefusal::EmptyProgram)
    );
}

#[test]
fn a_valid_entry_passes_when_enabled() {
    assert!(check(&cmd("echo", &["hi"], true), true).is_ok());
}

// ── no shell means no injection ──────────────────────────────────────

/// The property the whole design rests on: arguments are passed
/// verbatim to the program, never to a shell. A metacharacter is
/// therefore just a character.
#[test]
fn metacharacters_are_data_not_syntax() {
    let out = run(&cmd(
        "echo",
        &["hello; touch /tmp/pt-should-not-exist"],
        true,
    ));
    assert_eq!(
        out.as_deref(),
        Some("hello; touch /tmp/pt-should-not-exist"),
        "the semicolon must be echoed, not executed"
    );
    assert!(
        !std::path::Path::new("/tmp/pt-should-not-exist").exists(),
        "a shell ran when none should have"
    );
}

// ── output handling ──────────────────────────────────────────────────

#[test]
fn stdout_comes_back_when_insertion_is_on() {
    assert_eq!(
        run(&cmd("echo", &["poltertype"], true)).as_deref(),
        Some("poltertype")
    );
}

#[test]
fn nothing_comes_back_when_insertion_is_off() {
    assert_eq!(run(&cmd("echo", &["poltertype"], false)), None);
}

/// A program that writes to stdout and then fails should not have its
/// message typed into the user's document.
#[test]
fn a_failing_command_types_nothing() {
    assert_eq!(run(&cmd("sh", &["-c", "echo oops; exit 3"], true)), None);
}

#[test]
fn a_missing_program_is_survivable() {
    assert_eq!(run(&cmd("poltertype-no-such-binary-xyz", &[], true)), None);
}

/// A hung command must not hold the worker thread forever. Uses a
/// sleep just past the timeout so the test stays quick.
#[test]
fn a_hanging_command_is_killed() {
    let started = Instant::now();
    let out = run(&cmd("sleep", &["30"], true));
    assert_eq!(out, None);
    assert!(
        started.elapsed() < RUN_TIMEOUT + Duration::from_secs(3),
        "should have given up near the timeout, took {:?}",
        started.elapsed()
    );
}

// ── sanitising ───────────────────────────────────────────────────────

/// Typing is not printing. A newline in the middle of inserted text
/// submits a chat message or runs a shell line.
#[test]
fn newlines_never_reach_the_keyboard() {
    let s = sanitise_output(b"first\nsecond\r\nthird");
    assert!(!s.contains('\n') && !s.contains('\r'), "got {s:?}");
    assert!(s.contains("first") && s.contains("third"));
}

#[test]
fn the_trailing_newline_every_command_emits_is_dropped() {
    assert_eq!(sanitise_output(b"2026-08-02\n"), "2026-08-02");
}

#[test]
fn escape_sequences_are_stripped() {
    let s = sanitise_output(b"\x1b[31mred\x1b[0m");
    assert!(!s.contains('\x1b'), "got {s:?}");
    assert!(s.contains("red"));
}

#[test]
fn output_is_capped() {
    let big = vec![b'x'; MAX_OUTPUT_BYTES * 4];
    assert!(sanitise_output(&big).len() <= MAX_OUTPUT_BYTES);
}

/// Truncation must not split a multi-byte character into invalid
/// UTF-8 — the result is typed, and half a character is not typeable.
#[test]
fn truncation_respects_character_boundaries() {
    let mut raw = Vec::new();
    while raw.len() < MAX_OUTPUT_BYTES + 8 {
        raw.extend_from_slice("привіт".as_bytes());
    }
    let s = sanitise_output(&raw);
    assert!(s.len() <= MAX_OUTPUT_BYTES);
    assert!(!s.is_empty());
    // Round-tripping proves it is valid UTF-8 with no partial char.
    assert_eq!(s, String::from_utf8_lossy(s.as_bytes()));
}

#[test]
fn invalid_utf8_does_not_panic() {
    let _ = sanitise_output(&[0xFF, 0xFE, b'o', b'k']);
}
