#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use poltertype_core::plugins::{DiscoveredExtension, ExtensionManifest, PluginCommand};

use super::*;

/// The interpreter these fixtures drive, and the flag that makes it
/// take a command string.
///
/// Chosen at runtime rather than with `#[cfg(target_os)]`: what differs
/// is a *value*, not an API, and this crate holds no platform
/// conditionals.
///
/// Getting it wrong is not a failing test but a **passing** one: half
/// this suite asserts that something is *not* running, which is
/// trivially true when the process never started.
fn shell() -> (PathBuf, &'static str) {
    if cfg!(windows) {
        (PathBuf::from("cmd.exe"), "/C")
    } else {
        (PathBuf::from("/bin/sh"), "-c")
    }
}

/// A command that outlives anything this suite waits for.
///
/// Windows has no `sleep` binary; `ping` against the loopback with a
/// one-second interval is the usual stand-in, and unlike `timeout` it
/// does not need a console to read from — these children are spawned
/// with a null stdin.
fn stay_alive() -> &'static str {
    if cfg!(windows) {
        "ping -n 31 127.0.0.1 >nul"
    } else {
        "sleep 30"
    }
}

/// A command that exits immediately with the given code.
fn exit_with(code: u8) -> String {
    format!("exit {code}")
}

/// An extension whose "program" is a shell that just waits, so the
/// supervisor can be exercised without a plug-in existing.
fn sleeper(service: bool) -> DiscoveredExtension {
    let (exe, flag) = shell();
    DiscoveredExtension {
        id: "test-sleeper".to_owned(),
        name: "Sleeper".to_owned(),
        version: "0".to_owned(),
        dir: std::env::temp_dir(),
        exe,
        manifest: ExtensionManifest {
            exe: "sh".to_owned(),
            service_args: if service {
                vec![flag.to_owned(), stay_alive().to_owned()]
            } else {
                Vec::new()
            },
            commands: vec![PluginCommand {
                id: "quick".to_owned(),
                label: "Quick".to_owned(),
                args: vec![flag.to_owned(), exit_with(0)],
            }],
            ..ExtensionManifest::default()
        },
        development: true,
    }
}

/// The same, with the service replaced by one that exits with `code`.
fn dies_with(code: u8) -> DiscoveredExtension {
    let (_, flag) = shell();
    let mut ext = sleeper(true);
    ext.manifest.service_args = vec![flag.to_owned(), exit_with(code)];
    ext
}

#[test]
fn a_service_is_started_and_then_stopped() {
    let mut sup = Supervisor::new();
    sup.start_all(std::slice::from_ref(&sleeper(true)));
    assert!(sup.is_running("test-sleeper"), "service should be running");

    sup.stop_all();
    assert!(!sup.is_running("test-sleeper"), "stop_all must clear it");
}

#[test]
fn an_extension_with_no_service_starts_nothing() {
    // Most plug-ins will be command-only; starting a process for them
    // would be a process doing nothing forever.
    let mut sup = Supervisor::new();
    sup.start_all(std::slice::from_ref(&sleeper(false)));
    assert!(!sup.is_running("test-sleeper"));
}

#[test]
fn a_service_that_exits_is_reaped_and_reported_once() {
    let ext = dies_with(3);

    let mut sup = Supervisor::new();
    sup.start_all(std::slice::from_ref(&ext));

    // Give the child a moment to be done; poll rather than assume.
    let mut gone = Vec::new();
    for _ in 0..50 {
        gone = sup.reap();
        if !gone.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let ids: Vec<&str> = gone.iter().map(|d| d.id.as_str()).collect();
    assert_eq!(ids, vec!["test-sleeper"]);
    // Whatever the platform calls it, the code the service died with
    // has to survive into the line the user is shown.
    assert!(gone[0].why.contains('3'), "{:?}", gone[0].why);
    assert!(!sup.is_running("test-sleeper"));
    // A dead service is reported once, not on every heartbeat.
    assert!(sup.reap().is_empty());
}

/// What a plug-in said last is the whole reason its log is read, so the
/// reading is tested directly: the reap path above cannot produce a
/// crash message on demand, and a plug-in dying with something to say
/// is exactly the case that matters.
#[test]
fn the_last_thing_a_plugin_said_is_what_gets_quoted() {
    let dir = std::env::temp_dir().join(format!("poltertype-tail-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("plugin.log");

    // Trailing blank lines are what a tidy program ends with; the
    // message is the line before them.
    std::fs::write(&path, "starting up\nError: the keyring said no\n\n\n").unwrap();
    assert_eq!(
        last_line(&path).as_deref(),
        Some("Error: the keyring said no")
    );

    // Nothing said at all: no quote, not an empty one.
    std::fs::write(&path, "").unwrap();
    assert_eq!(last_line(&path), None);
    std::fs::write(&path, "\n  \n").unwrap();
    assert_eq!(last_line(&path), None);

    // A line too long for a notification is cut, not passed through.
    std::fs::write(&path, format!("{}\n", "x".repeat(LOG_LINE_CHARS + 50))).unwrap();
    let cut = last_line(&path).unwrap();
    assert_eq!(cut.chars().count(), LOG_LINE_CHARS + 1, "{cut}");
    assert!(cut.ends_with('…'));

    // A plug-in that logged all day is not read into memory; only the
    // end of the file is, and the answer still comes from the end.
    let mut big = "noise\n".repeat(4 * LOG_TAIL_BYTES as usize / 6);
    big.push_str("the last word\n");
    std::fs::write(&path, big).unwrap();
    assert_eq!(last_line(&path).as_deref(), Some("the last word"));

    let _ = std::fs::remove_file(&path);
}

/// A service with no log file still reports its death — the file is a
/// nicety, the reaping is not.
#[test]
fn a_service_whose_log_could_not_be_opened_is_still_reaped() {
    let mut sup = Supervisor::new();
    sup.start_all(std::slice::from_ref(&dies_with(2)));
    for r in &mut sup.running {
        r.log = None;
    }
    let mut gone = Vec::new();
    for _ in 0..50 {
        gone = sup.reap();
        if !gone.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(gone.len(), 1, "the death must be reported without a log");
    assert!(!gone[0].why.is_empty());
}

#[test]
fn a_dead_service_is_not_restarted() {
    // An automatic restart would turn a plug-in that crashes on start
    // into a fork bomb, and would hide the failure the user needs to
    // see.
    let ext = dies_with(1);
    let mut sup = Supervisor::new();
    sup.start_all(std::slice::from_ref(&ext));
    for _ in 0..50 {
        if !sup.reap().is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(!sup.is_running("test-sleeper"));
    assert!(sup.reap().is_empty(), "nothing should have come back");
}

#[test]
fn a_declared_command_runs() {
    assert!(run_command(&sleeper(false), "quick").is_ok());
}

#[test]
fn an_undeclared_command_is_refused_rather_than_guessed() {
    // The manifest is the whole list of what a plug-in may be asked to
    // do; a caller inventing a command must not reach the process.
    let err = run_command(&sleeper(false), "rm-rf").unwrap_err();
    assert!(err.contains("rm-rf"), "{err}");
}

#[test]
fn stopping_twice_is_harmless() {
    let mut sup = Supervisor::new();
    sup.start_all(std::slice::from_ref(&sleeper(true)));
    sup.stop_all();
    sup.stop_all();
    assert!(!sup.is_running("test-sleeper"));
}

/// The fixture itself has to work, or most of this file passes by
/// accident. Asserts the positive directly, so a platform where the
/// interpreter is missing or takes a different flag fails here — with
/// an obvious message — rather than quietly turning six tests into
/// assertions about a process that was never created.
#[test]
fn the_fixture_can_actually_start_a_process() {
    let (exe, flag) = shell();
    let status = std::process::Command::new(&exe)
        .arg(flag)
        .arg(exit_with(0))
        .status();
    assert!(
        matches!(&status, Ok(s) if s.success()),
        "fixture interpreter {exe:?} {flag} did not run: {status:?}"
    );
}

/// A plug-in that declares `stop` is asked before it is killed.
///
/// The observable is a file the stop command creates: nothing else
/// proves the command ran in the plug-in's own program rather than
/// being decided about and skipped. This is the whole graceful-shutdown
/// path on Windows — see `STOP_COMMAND`.
#[test]
fn a_declared_stop_command_is_run_before_the_kill() {
    let (_, flag) = shell();
    // `std::process::id()` alone is unique enough here. It used to also
    // fold in a `ThreadId`, which renders as `ThreadId(2)` — an
    // unquoted `(` is a subshell to `/bin/sh -c`, so the marker path
    // broke the very command meant to write it. Dropping it removes the
    // one character class this string can never safely contain.
    let marker =
        std::env::temp_dir().join(format!("poltertype-stop-{}.marker", std::process::id()));
    let _ = std::fs::remove_file(&marker);

    let mut ext = sleeper(true);
    ext.manifest.commands.push(PluginCommand {
        id: STOP_COMMAND.to_owned(),
        label: "Stop".to_owned(),
        args: vec![
            flag.to_owned(),
            format!("echo stopped> {}", marker.display()),
        ],
    });
    assert!(declares_stop(&ext), "the fixture must declare the command");

    let mut sup = Supervisor::new();
    sup.start_all(std::slice::from_ref(&ext));
    assert!(sup.is_running("test-sleeper"), "service should be running");

    sup.stop_all();

    // The command is spawned, not waited on, so give it a moment.
    let mut ran = false;
    for _ in 0..50 {
        if marker.exists() {
            ran = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let _ = std::fs::remove_file(&marker);
    assert!(ran, "stop_all did not run the declared stop command");
    assert!(!sup.is_running("test-sleeper"));
}

/// A plug-in that declares no stop command is not asked, and stopping
/// it still works — the kill is what it always was.
#[test]
fn a_plugin_without_a_stop_command_is_simply_killed() {
    let ext = sleeper(true);
    assert!(!declares_stop(&ext));

    let mut sup = Supervisor::new();
    sup.start_all(std::slice::from_ref(&ext));
    sup.stop_all();
    assert!(!sup.is_running("test-sleeper"));
}
