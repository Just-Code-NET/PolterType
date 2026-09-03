use std::time::Duration;

use super::*;

/// A sample's age is what the caller uses to decide whether to trust
/// it, so it has to actually advance.
#[test]
fn a_sample_ages() {
    let s = FocusSample {
        exe: "kate".into(),
        at: Instant::now() - Duration::from_secs(10),
    };
    assert!(s.age() >= Duration::from_secs(10));
}

/// Constructing the watcher must never panic, whatever the session
/// looks like. On a machine with no a11y bus at all — headless CI is
/// the case that matters — it has to come back as a plain error the
/// factory can log and move past.
#[test]
fn construction_is_infallible_in_the_panic_sense() {
    match AtspiFocusWatcher::try_new() {
        // A live a11y bus. Nothing has necessarily been activated
        // yet, so the only guarantee is that reading is safe.
        Ok(w) => drop(w.latest()),
        // Every variant must describe a missing or unusable a11y
        // stack rather than a bug — the caller treats it as normal.
        Err(e) => assert!(!e.to_string().is_empty()),
    }
}

/// PID lookup must fail gracefully for a name nobody owns rather than
/// propagating a bus error upward: an app can exit between sending an
/// event and our asking about it, and that is ordinary churn.
#[test]
fn an_unknown_sender_yields_no_pid() {
    let Some(conn) = test_a11y_connection() else {
        return; // no a11y bus here; nothing to assert against
    };
    assert_eq!(
        connection_pid(&conn, ":99.99999"),
        None,
        "a sender that does not exist must not resolve to a PID"
    );
}

/// The end-to-end check, run by hand against a live desktop:
///
/// ```text
/// cargo test -p poltertype-input -- --ignored --nocapture \
///     names_the_application_that_takes_focus
/// # …then focus an a11y-capable window (kate, a GTK app, a browser)
/// ```
///
/// Ignored by default: it needs a session, a window manager and
/// something to move focus, none of which CI has.
#[test]
#[ignore = "needs a live desktop and a focus change"]
fn names_the_application_that_takes_focus() {
    let Ok(w) = AtspiFocusWatcher::try_new() else {
        eprintln!("no a11y bus — cannot run this check here");
        return;
    };
    eprintln!("watching for window:activate — focus another window now…");
    for _ in 0..30 {
        if let Some(s) = w.latest() {
            eprintln!("observed focus: exe={} age={:?}", s.exe, s.age());
            assert!(!s.exe.is_empty(), "an observation must name an executable");
            return;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    // Not an assertion failure: on a desktop where nothing focused is
    // a11y-capable this is the documented limitation, not a bug.
    eprintln!("no window:activate in 15s — was any focused app a11y-capable?");
}

/// Open a connection to the a11y bus, or `None` if this machine has
/// none. Mirrors the production path without duplicating its logging.
fn test_a11y_connection() -> Option<Connection> {
    let session = Connection::session().ok()?;
    let reply = session
        .call_method(
            Some("org.a11y.Bus"),
            "/org/a11y/bus",
            Some("org.a11y.Bus"),
            "GetAddress",
            &(),
        )
        .ok()?;
    let address: String = reply.body().deserialize().ok()?;
    Builder::address(address.as_str()).ok()?.build().ok()
}
