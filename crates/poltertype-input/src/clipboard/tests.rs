use super::*;

/// A write has to outlive the call that made it.
///
/// The one property selection conversion rests on, and the one it
/// was missing: the converted text is staged, a paste chord goes
/// out, and the focused application reads the clipboard some
/// milliseconds later — from whichever process still owns it. A
/// write that dies with its handle reads back as an empty
/// clipboard, which is what issue #51 was, on every Linux backend.
///
/// `cargo test -p poltertype-input -- --ignored --nocapture a_write_outlives_the_call`
///
/// Ignored because it needs a real session; there is no clipboard
/// on a CI runner, and a skip that passes would say nothing.
#[test]
#[ignore = "needs a real desktop session's clipboard"]
fn a_write_outlives_the_call() {
    let Ok(cb) = clipboard() else {
        println!("no windowless clipboard in this session — nothing measured");
        return;
    };
    let before = cb.text().ok().flatten();
    let marker = format!("poltertype-durability-{}", std::process::id());
    let staged = cb.set_text(&marker);
    assert!(staged.is_ok(), "could not stage the marker: {staged:?}");

    // Read through a *fresh* handle, and after the pause a paste
    // really takes: reading back through the one that wrote would
    // prove nothing, since that handle is the thing under test.
    std::thread::sleep(std::time::Duration::from_millis(400));
    let read = cb.text();

    // Before the assertion, so a failure does not also walk off
    // with the session's clipboard.
    if let Some(prev) = before {
        let _ = cb.set_text(&prev);
    }
    assert_eq!(
        read.ok().flatten().as_deref(),
        Some(marker.as_str()),
        "the staged text must still be there when the paste asks for it"
    );
}

/// Reports whether *this* session lets a windowless process reach
/// the clipboard, and round-trips a marker through it if so.
///
/// `cargo test -p poltertype-input -- --ignored --nocapture clipboard_of_this_session`
///
/// Ignored because the answer is a property of the machine, not of
/// the code — and it is the answer the desktop matrix collects, one
/// session at a time. Asserting anything here would fail every run
/// on GNOME, where the honest result is "unavailable".
#[test]
#[ignore = "reports this session's real clipboard access; nothing to assert"]
fn clipboard_of_this_session() {
    let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "?".into());
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "?".into());
    match clipboard() {
        Err(gap) => {
            println!("CLIPBOARD session={session} desktop={desktop} available=no gap={gap:?}");
            println!("  reads as: {gap}");
        }
        Ok(cb) => {
            let before = cb.text();
            let marker = format!("poltertype-probe-{}", std::process::id());
            let wrote = cb.set_text(&marker);
            std::thread::sleep(std::time::Duration::from_millis(300));
            let read = cb.text();
            let ok = matches!(&read, Ok(Some(t)) if *t == marker);
            println!(
                "CLIPBOARD session={session} desktop={desktop} available=yes \
                 roundtrip={ok} wrote={:?} read_ok={}",
                wrote.is_ok(),
                read.is_ok()
            );
            // Put back whatever was there, so a sweep does not walk
            // off with the session's clipboard.
            if let Ok(Some(prev)) = before {
                let _ = cb.set_text(&prev);
            }
        }
    }
}
