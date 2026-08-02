#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use poltertype_core::plugins::{DiscoveredExtension, ExtensionManifest, PluginCommand};

use super::*;

/// An extension whose "program" is a shell that just waits, so the
/// supervisor can be exercised without a plug-in existing.
fn sleeper(service: bool) -> DiscoveredExtension {
    DiscoveredExtension {
        id: "test-sleeper".to_owned(),
        name: "Sleeper".to_owned(),
        version: "0".to_owned(),
        dir: std::env::temp_dir(),
        exe: PathBuf::from("/bin/sh"),
        manifest: ExtensionManifest {
            exe: "sh".to_owned(),
            service_args: if service {
                vec!["-c".to_owned(), "sleep 30".to_owned()]
            } else {
                Vec::new()
            },
            commands: vec![PluginCommand {
                id: "quick".to_owned(),
                label: "Quick".to_owned(),
                args: vec!["-c".to_owned(), "true".to_owned()],
            }],
            ..ExtensionManifest::default()
        },
        development: true,
    }
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
    let mut ext = sleeper(true);
    ext.manifest.service_args = vec!["-c".to_owned(), "exit 3".to_owned()];

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
    assert_eq!(gone, vec!["test-sleeper".to_owned()]);
    assert!(!sup.is_running("test-sleeper"));
    // A dead service is reported once, not on every heartbeat.
    assert!(sup.reap().is_empty());
}

#[test]
fn a_dead_service_is_not_restarted() {
    // An automatic restart would turn a plug-in that crashes on start
    // into a fork bomb, and would hide the failure the user needs to
    // see.
    let mut ext = sleeper(true);
    ext.manifest.service_args = vec!["-c".to_owned(), "exit 1".to_owned()];
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
