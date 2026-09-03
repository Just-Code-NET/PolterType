//! Starting plug-in services, reaping the ones that exit, and stopping
//! all of them again.

use poltertype_core::plugins::DiscoveredExtension;
use tracing::{info, warn};

use crate::plugins::consts::STOP_COMMAND;
use crate::plugins::types::Departed;

use super::commands::{declares_stop, run_command};
use super::process::{last_line, service_log, spawn};
use super::state::Supervisor;
use super::types::Running;

impl Supervisor {
    /// Start the service half of every extension that has one.
    pub fn start_all(&mut self, extensions: &[DiscoveredExtension]) {
        for ext in extensions {
            if ext.manifest.service_args.is_empty() {
                continue;
            }
            let log = service_log(&ext.id);
            match spawn(&ext.exe, &ext.manifest.service_args, &ext.dir, log.as_ref()) {
                Ok(child) => {
                    info!(
                        id = %ext.id,
                        pid = child.id(),
                        development = ext.development,
                        log = ?log.as_ref().map(|(path, _)| path),
                        "plug-in service started"
                    );
                    self.running.push(Running {
                        id: ext.id.clone(),
                        child,
                        ext: ext.clone(),
                        log: log.map(|(path, _)| path),
                    });
                }
                Err(e) => warn!(id = %ext.id, "could not start plug-in service: {e}"),
            }
        }
    }

    /// Report any service that has exited since the last check, and
    /// forget it.
    ///
    /// Called from the plug-in heartbeat: nothing else would notice a
    /// death, because the tray entries behind a service are one-shot
    /// commands that work whether it is alive or not.
    ///
    /// Reaping is also what stops the corpse being a zombie.
    pub fn reap(&mut self) -> Vec<Departed> {
        let mut gone = Vec::new();
        self.running.retain_mut(|r| match r.child.try_wait() {
            Ok(Some(status)) => {
                let last = r.log.as_deref().and_then(last_line);
                warn!(
                    id = %r.id,
                    ?status,
                    log = ?r.log,
                    last = last.as_deref().unwrap_or(""),
                    "plug-in service exited"
                );
                gone.push(Departed {
                    id: r.id.clone(),
                    why: match &last {
                        Some(line) => format!("{status} — {line}"),
                        None => status.to_string(),
                    },
                });
                false
            }
            Ok(None) => true,
            Err(e) => {
                warn!(id = %r.id, "cannot check on plug-in service: {e}");
                gone.push(Departed {
                    id: r.id.clone(),
                    why: format!("cannot check on it: {e}"),
                });
                false
            }
        });
        gone
    }

    /// Whether any service is being supervised at all — the heartbeat
    /// that reaps them has no reason to run otherwise.
    pub fn has_services(&self) -> bool {
        !self.running.is_empty()
    }

    /// Ask every service to stop, then make sure it did. Politeness
    /// costs a moment and buys an in-flight buffer the chance to flush;
    /// a plug-in that ignores the request still gets killed.
    pub fn stop_all(&mut self) {
        for r in &mut self.running {
            // The plug-in's own idea of stopping, first and on every
            // platform. One that declares nothing is not asked.
            if declares_stop(&r.ext) {
                match run_command(&r.ext, STOP_COMMAND) {
                    Ok(()) => info!(id = %r.id, "asked the plug-in to stop"),
                    Err(e) => warn!(id = %r.id, "declared stop command failed: {e}"),
                }
            }
            // And the OS's own way of asking, where there is one. Both
            // are requests, so the kill below is not optional.
            poltertype_shell::request_stop(r.child.id());
        }
        // A grace period, then whatever is left is killed. Short on
        // purpose: a user clicking Quit should not have to wait for
        // somebody else's shutdown code.
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
