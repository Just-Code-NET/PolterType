//! Running plug-in programs, and stopping them again.
//!
//! A plug-in is a separate process, never code loaded into this one.
//! That is what keeps the process holding the global keyboard hook out
//! of the blast radius of third-party code: a plug-in that panics,
//! deadlocks or is outright malicious costs the user that plug-in, not
//! their keyboard.
//!
//! Two ways to run one, and they are different in kind:
//!
//! * a **service** — the long-running half, started when PolterType
//!   starts and stopped when it quits;
//! * a **command** — a one-shot invocation behind a menu entry or a
//!   button, which runs, does its thing and exits.
//!
//! ## What this deliberately does not do
//!
//! **No restart loop.** A service that dies stays dead until the user
//! asks again. Restarting it automatically would turn a plug-in that
//! crashes on startup into a fork bomb that also fills the log, and it
//! would hide exactly the failure the user needs to see.
//!
//! **No shell.** Arguments come from the manifest as a list and are
//! passed as a list. There is no string to quote, so there is nothing
//! to quote wrongly.
//!
//! **No inherited standard input.** A plug-in gets a null stdin, so it
//! can never sit waiting on a terminal that a tray app does not have.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use poltertype_core::plugins::DiscoveredExtension;
use tracing::{info, warn};

/// One running service, and enough to identify it in a log line.
struct Running {
    id: String,
    child: Child,
}

/// Owns every plug-in process this app started.
#[derive(Default)]
pub struct Supervisor {
    running: Vec<Running>,
}

impl Supervisor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start the service half of every extension that has one.
    pub fn start_all(&mut self, extensions: &[DiscoveredExtension]) {
        for ext in extensions {
            if ext.manifest.service_args.is_empty() {
                continue;
            }
            match spawn(&ext.exe, &ext.manifest.service_args, &ext.dir) {
                Ok(child) => {
                    info!(
                        id = %ext.id,
                        pid = child.id(),
                        development = ext.development,
                        "plug-in service started"
                    );
                    self.running.push(Running {
                        id: ext.id.clone(),
                        child,
                    });
                }
                Err(e) => warn!(id = %ext.id, "could not start plug-in service: {e}"),
            }
        }
    }

    /// Report any service that has exited since the last check, and
    /// forget it. Called from the tray's heartbeat so a plug-in dying
    /// shows up in the log at the moment it happens rather than at
    /// shutdown.
    pub fn reap(&mut self) -> Vec<String> {
        let mut gone = Vec::new();
        self.running.retain_mut(|r| match r.child.try_wait() {
            Ok(Some(status)) => {
                warn!(id = %r.id, ?status, "plug-in service exited");
                gone.push(r.id.clone());
                false
            }
            Ok(None) => true,
            Err(e) => {
                warn!(id = %r.id, "cannot check on plug-in service: {e}");
                gone.push(r.id.clone());
                false
            }
        });
        gone
    }

    /// Ask every service to stop, then make sure it did.
    ///
    /// A plug-in is asked politely first because the one this was
    /// written for has an in-flight buffer to flush on the way out —
    /// and a plug-in that ignores the request still gets killed, so
    /// being polite costs a moment, not a guarantee.
    pub fn stop_all(&mut self) {
        for r in &mut self.running {
            poltertype_shell::request_stop(r.child.id());
        }
        // A grace period, then whatever is left is killed. Deliberately
        // short: this runs on the way out of a tray app, and a user
        // clicking Quit should not have to wait for somebody else's
        // shutdown code.
        std::thread::sleep(std::time::Duration::from_millis(400));
        for r in &mut self.running {
            match r.child.try_wait() {
                Ok(Some(_)) => info!(id = %r.id, "plug-in service stopped"),
                _ => {
                    warn!(id = %r.id, "plug-in service did not stop; killing it");
                    let _ = r.child.kill();
                    let _ = r.child.wait();
                }
            }
        }
        self.running.clear();
    }

    pub fn is_running(&self, id: &str) -> bool {
        self.running.iter().any(|r| r.id == id)
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        // A plug-in outliving the app that started it would be a
        // keystroke-reading process with no visible owner.
        self.stop_all();
    }
}

/// Run one of a plug-in's declared commands and leave it to finish on
/// its own. Used by tray entries and pane buttons, which must return
/// immediately — the menu is on the UI thread.
pub fn run_command(ext: &DiscoveredExtension, command_id: &str) -> Result<(), String> {
    let cmd = ext
        .manifest
        .commands
        .iter()
        .find(|c| c.id == command_id)
        .ok_or_else(|| format!("{} declares no command {command_id:?}", ext.id))?;

    spawn(&ext.exe, &cmd.args, &ext.dir)
        .map(|child| {
            info!(id = %ext.id, command = %command_id, pid = child.id(), "plug-in command started");
            // Deliberately not waited on: these are user-facing actions
            // that may take seconds, and the tray must not block. The
            // child is reparented when it outlives us, which for a
            // one-shot command is the right outcome.
        })
        .map_err(|e| format!("could not run {command_id:?}: {e}"))
}

fn spawn(exe: &PathBuf, args: &[String], dir: &PathBuf) -> std::io::Result<Child> {
    Command::new(exe)
        .args(args)
        // The plug-in's own directory, so a relative path in its
        // config means what its author expected.
        .current_dir(dir)
        .stdin(Stdio::null())
        .spawn()
}

#[cfg(test)]
#[path = "supervisor_tests.rs"]
mod tests;
