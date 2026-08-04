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

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use poltertype_core::plugins::DiscoveredExtension;
use tracing::{info, warn};

/// One running service, and enough to identify it in a log line.
struct Running {
    id: String,
    child: Child,
    /// The extension this process came from, kept so it can be asked to
    /// stop the way *it* declared. Cloning it costs a few strings once
    /// per plug-in at startup.
    ext: DiscoveredExtension,
}

/// The command id a plug-in may declare to be told "wind up now".
///
/// Reserved rather than invented per plug-in: the supervisor has to
/// know the name to call it without being configured, and a plug-in
/// that does not declare it simply is not asked.
///
/// This is how a graceful stop works on **every** platform, and it
/// exists because the per-OS mechanisms do not. Unix has SIGTERM, which
/// is real but only half the story: a plug-in still has to install a
/// handler for it. Windows has no signal at all, and the console
/// control event that stands in for one was measured here and refused —
/// addressed to the child's process group it returned success and did
/// nothing, and addressed to the whole console it killed the sender.
/// Neither outcome is acceptable in the process holding the global
/// keyboard hook. See `docs/DECISIONS.md`.
///
/// A declared command has none of those problems: it is the plug-in's
/// own program, run the way every other plug-in action is run, and what
/// "stop cleanly" means is the plug-in author's to define.
pub const STOP_COMMAND: &str = "stop";

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
                        ext: ext.clone(),
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
            // The plug-in's own idea of stopping, first and on every
            // platform. A plug-in that declares nothing is not asked,
            // and falls through to the two lines below exactly as
            // before.
            if declares_stop(&r.ext) {
                match run_command(&r.ext, STOP_COMMAND) {
                    Ok(()) => info!(id = %r.id, "asked the plug-in to stop"),
                    Err(e) => warn!(id = %r.id, "declared stop command failed: {e}"),
                }
            }
            // And the OS's own way of asking, where there is one. Both
            // are requests; neither is guaranteed, which is why the
            // kill below is not optional.
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

/// Whether this extension declared the reserved stop command.
pub fn declares_stop(ext: &DiscoveredExtension) -> bool {
    ext.manifest.commands.iter().any(|c| c.id == STOP_COMMAND)
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

/// Ask a plug-in what state it is in, for the tray to reflect.
///
/// Unlike [`run_command`] this **is** waited on — the answer is the
/// point — so it carries a deadline. A plug-in that hangs here would
/// otherwise freeze the tray menu, and a stale tick is a far smaller
/// problem than a frozen tray.
///
/// Output is one `key=value` per line. Anything else on a line is
/// ignored rather than rejected, so a plug-in may print a human-facing
/// summary alongside without breaking this.
///
/// `None` means the plug-in could not be asked at all — it is not
/// installed, it crashed, it timed out. That is a different thing from
/// answering without mentioning a particular key, and the menu says so
/// differently, because one of them is worth investigating and the
/// other is normal.
pub fn read_state(ext: &DiscoveredExtension) -> Option<HashMap<String, String>> {
    if ext.manifest.state_args.is_empty() {
        return None;
    }

    let stdout = match state_output(ext) {
        Ok(out) => out,
        Err(e) => {
            warn!(id = %ext.id, "cannot read plug-in state: {e}");
            return None;
        }
    };

    let mut state = HashMap::new();
    for line in stdout.lines() {
        if let Some((key, value)) = line.split_once('=') {
            let (key, value) = (key.trim(), value.trim());
            if !key.is_empty() {
                state.insert(key.to_owned(), value.to_owned());
            }
        }
    }
    Some(state)
}

/// Run the state command and return its stdout, or give up.
///
/// The deadline is the reason this is not one `output()` call. That
/// call waits forever, and this one runs on the UI thread while a menu
/// is being drawn — a plug-in that blocks on a lock, a dialog or a
/// dead socket would take the tray with it. A stale tick is a far
/// smaller problem than a tray that stops responding, so the process is
/// killed and the previous state left alone.
fn state_output(ext: &DiscoveredExtension) -> Result<String, String> {
    use std::io::Read as _;

    let mut cmd = Command::new(&ext.exe);
    cmd.args(&ext.manifest.state_args)
        .current_dir(&ext.dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // This one runs every time the tray menu is drawn, so a console
    // window here would flash on every click, not once at startup.
    poltertype_shell::configure_child(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;

    // Drained on its own thread: a plug-in printing more than a pipe
    // buffer would block forever if we polled without reading.
    let mut pipe = child.stdout.take().ok_or("no stdout")?;
    let reader = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = pipe.read_to_string(&mut buf);
        buf
    });

    let deadline = std::time::Instant::now() + STATE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let out = reader.join().unwrap_or_default();
                return if status.success() {
                    Ok(out)
                } else {
                    Err(format!("state command exited {status}"))
                };
            }
            Ok(None) if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "state command did not answer within {}ms",
                    STATE_TIMEOUT.as_millis()
                ));
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(10)),
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// How long a plug-in gets to report its state. Short on purpose: this
/// blocks the thread that draws the menu.
const STATE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(1_500);

fn spawn(exe: &PathBuf, args: &[String], dir: &PathBuf) -> std::io::Result<Child> {
    let mut cmd = Command::new(exe);
    cmd.args(args)
        // The plug-in's own directory, so a relative path in its
        // config means what its author expected.
        .current_dir(dir)
        .stdin(Stdio::null());
    // A tray app owns no console, so a console child would be handed a
    // window of its own. See `poltertype_shell::configure_child`.
    poltertype_shell::configure_child(&mut cmd);
    cmd.spawn()
}

#[cfg(test)]
#[path = "supervisor_tests.rs"]
mod tests;
