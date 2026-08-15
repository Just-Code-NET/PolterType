//! Running plug-in programs, and stopping them again.
//!
//! A plug-in is a separate process, never code loaded into this one —
//! see `docs/ARCHITECTURE.md` § Plug-ins for why, and for the four
//! things this deliberately does not do (no restart loop, no shell, no
//! inherited stdin, no filtering of a plug-in's own output).
//!
//! A **service** runs for as long as PolterType does; a **command** is
//! a one-shot invocation behind a menu entry. A service's output goes
//! to `logs/plugin-<id>.log`, truncated at every start, and the tail of
//! that file is what [`Supervisor::reap`] quotes to the user.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use poltertype_core::plugins::DiscoveredExtension;
use poltertype_core::settings::SettingsStore;
use tracing::{info, warn};

/// One running service, and enough to identify it in a log line.
struct Running {
    id: String,
    child: Child,
    /// The extension this process came from, kept so it can be asked to
    /// stop the way *it* declared. Cloning it costs a few strings once
    /// per plug-in at startup.
    ext: DiscoveredExtension,
    /// Where this service's own output went, if we managed to open a
    /// file for it. Read only when the service is gone.
    log: Option<PathBuf>,
}

/// A service that has exited, and the shortest true answer to "why".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Departed {
    pub id: String,
    /// Exit status, plus the last thing the plug-in said if it said
    /// anything. Already one line, already bounded — it goes in a
    /// notification.
    pub why: String,
}

/// The command id a plug-in may declare to be told "wind up now".
///
/// Reserved rather than invented per plug-in, because the supervisor
/// has to know the name without being configured; a plug-in that does
/// not declare it is simply not asked.
///
/// This is how a graceful stop works on **every** platform, and it
/// exists because the per-OS mechanisms do not: SIGTERM still needs the
/// plug-in to install a handler, and Windows' console control event was
/// measured here and refused — see `docs/DECISIONS.md`. A declared
/// command has neither problem, and what "stop cleanly" means stays the
/// plug-in author's to define.
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
    /// Called from the plug-in heartbeat, so a service dying is noticed
    /// while it happens rather than at the user's next tray click — the
    /// mode is answered by a one-shot command that works whether the
    /// service is alive or not, so nothing else would notice.
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

    // No log file for a one-shot: it inherits, as it always has. The
    // service log exists because a service dies unobserved; a command
    // is something the user just clicked and is watching for.
    spawn(&ext.exe, &cmd.args, &ext.dir, None)
        .map(|child| {
            info!(id = %ext.id, command = %command_id, pid = child.id(), "plug-in command started");
            // Deliberately not waited on: these are user-facing actions
            // that may take seconds, and the tray must not block. The
            // child is reparented when it outlives us, which for a
            // one-shot command is the right outcome.
        })
        .map_err(|e| format!("could not run {command_id:?}: {e}"))
}

/// Run a declared command with one row id substituted into it — what a
/// per-row tray entry does.
///
/// Substitution is by whole argument: an argument that *is* `{id}`
/// becomes the row id, and nothing else is touched. Not string
/// interpolation, deliberately — a row id arrives from the plug-in's own
/// output, and pasting it into the middle of an argument is how it would
/// turn into a second flag.
pub fn run_command_for_row(
    ext: &DiscoveredExtension,
    command_id: &str,
    row_id: &str,
) -> Result<(), String> {
    let cmd = ext
        .manifest
        .commands
        .iter()
        .find(|c| c.id == command_id)
        .ok_or_else(|| format!("{} declares no command {command_id:?}", ext.id))?;
    let args: Vec<String> = cmd
        .args
        .iter()
        .map(|a| {
            if a == ROW_ID_PLACEHOLDER {
                row_id.to_owned()
            } else {
                a.clone()
            }
        })
        .collect();

    spawn(&ext.exe, &args, &ext.dir, None)
        .map(|child| {
            info!(id = %ext.id, command = %command_id, row = %row_id, pid = child.id(), "plug-in row command started");
        })
        .map_err(|e| format!("could not run {command_id:?}: {e}"))
}

/// The one argument a per-row command may have substituted.
pub const ROW_ID_PLACEHOLDER: &str = "{id}";

/// The same substitution as [`run_command_for_row`], but waiting for the
/// answer — what a pane's own row button needs.
///
/// A tray entry can afford to fire and forget: the menu closes and the
/// user is looking at whatever the plug-in does next. A button on a card
/// cannot. "Send this message now" that produces no visible outcome
/// leaves the one question it was pressed to answer — did it go? —
/// answered nowhere, and the pane's report only says so after somebody
/// thinks to press Refresh.
///
/// So this waits, off the UI thread, with a deadline of its own:
/// [`ACTION_TIMEOUT`] rather than a report's six seconds, because the
/// action behind such a button is not a query. Sending a standing
/// message opens a chat client, waits for the conversation to switch and
/// types a sentence at human speed.
pub fn run_command_for_row_waiting(
    ext: &DiscoveredExtension,
    command_id: &str,
    row_id: &str,
) -> Result<String, String> {
    let cmd = ext
        .manifest
        .commands
        .iter()
        .find(|c| c.id == command_id)
        .ok_or_else(|| format!("{} declares no command {command_id:?}", ext.id))?;
    let args: Vec<String> = cmd
        .args
        .iter()
        .map(|a| {
            if a == ROW_ID_PLACEHOLDER {
                row_id.to_owned()
            } else {
                a.clone()
            }
        })
        .collect();
    info!(id = %ext.id, command = %command_id, row = %row_id, "plug-in row action running");
    capture_output(ext, &args, ACTION_TIMEOUT, "row action")
}

/// Ask a plug-in for the rows of one of its runtime menus.
///
/// Waited on with a deadline, like the state read and for the same
/// reason: this runs while a menu is being built. An empty answer and a
/// failed one are both "no rows" here — the menu says so either way, and
/// the log carries the difference.
pub fn read_rows(ext: &DiscoveredExtension, command_id: &str) -> Vec<super::menu::MenuRow> {
    let Some(cmd) = ext.manifest.commands.iter().find(|c| c.id == command_id) else {
        warn!(id = %ext.id, "no command {command_id:?} to list from");
        return Vec::new();
    };
    match capture_output(ext, &cmd.args, STATE_TIMEOUT, "list command") {
        Ok(out) => super::menu::parse_rows(&out),
        Err(e) => {
            warn!(id = %ext.id, "cannot read plug-in menu rows: {e}");
            Vec::new()
        }
    }
}

/// Ask a plug-in what state it is in, for the tray to reflect.
///
/// Unlike [`run_command`] this **is** waited on, so it carries a
/// deadline — a plug-in that hangs here would freeze the tray menu.
///
/// Output is one `key=value` per line; anything else is ignored rather
/// than rejected, so a plug-in may print a human-facing summary too.
///
/// `None` means the plug-in could not be asked at all, which the menu
/// renders differently from an answer that omits a key: one is worth
/// investigating and the other is normal.
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
/// The deadline is why this is not one `output()` call: that waits for
/// ever, and this runs on the UI thread while a menu is being drawn. A
/// stale tick is a far smaller problem than a tray that stops
/// responding, so the process is killed and the previous state kept.
fn state_output(ext: &DiscoveredExtension) -> Result<String, String> {
    capture_output(
        ext,
        &ext.manifest.state_args,
        STATE_TIMEOUT,
        "state command",
    )
}

/// Run one of a plug-in's declared commands and return what it printed.
///
/// Separate from [`run_command`] rather than a flag on it: that one
/// must return before the child does, because it runs on the thread
/// drawing a menu. This one backs a pane that is *showing* an answer,
/// so it waits — off the UI thread, with its own deadline.
pub fn read_report(ext: &DiscoveredExtension, command_id: &str) -> Result<String, String> {
    let cmd = ext
        .manifest
        .commands
        .iter()
        .find(|c| c.id == command_id)
        .ok_or_else(|| format!("{} declares no command {command_id:?}", ext.id))?;
    capture_output(ext, &cmd.args, REPORT_TIMEOUT, "report command")
}

/// Run `args` against the plug-in and collect stdout, or give up.
/// Shared by the state read and the report read, so there is one place
/// that knows how to wait for a plug-in without being taken hostage.
fn capture_output(
    ext: &DiscoveredExtension,
    args: &[String],
    timeout: std::time::Duration,
    what: &str,
) -> Result<String, String> {
    use std::io::Read as _;

    let mut cmd = Command::new(&ext.exe);
    cmd.args(args)
        .current_dir(&ext.dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // The state read runs every time the tray menu is drawn, so a
    // console window here would flash on every click, not once at
    // startup.
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

    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let out = reader.join().unwrap_or_default();
                return if status.success() {
                    Ok(out)
                } else {
                    Err(format!("{what} exited {status}"))
                };
            }
            Ok(None) if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "{what} did not answer within {}ms",
                    timeout.as_millis()
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

/// How long a plug-in gets to produce a report. Longer than the state
/// read can afford, because nothing waits on the UI thread for it and
/// the answer may cost real work. Still bounded: a pane that says "it
/// did not answer" is honest, one that never renders is not.
const REPORT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(6);

/// How long a row's own button gets. Much longer than a report, because
/// it is not a query: the action behind one may steal focus, wait for
/// another application to switch conversation, and type a sentence at
/// human speed. Bounded all the same — a button that can hang for ever
/// is a pane that can never say what happened.
const ACTION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(90);

fn spawn(
    exe: &PathBuf,
    args: &[String],
    dir: &PathBuf,
    log: Option<&(PathBuf, std::fs::File)>,
) -> std::io::Result<Child> {
    let mut cmd = Command::new(exe);
    cmd.args(args)
        // The plug-in's own directory, so a relative path in its
        // config means what its author expected.
        .current_dir(dir)
        .stdin(Stdio::null());
    // Both streams to the same file, in the order the plug-in wrote
    // them. Without a log file we inherit, which is what this always
    // did and is still right when there *is* a terminal.
    if let Some((_, file)) = log {
        match (file.try_clone(), file.try_clone()) {
            (Ok(out), Ok(err)) => {
                cmd.stdout(Stdio::from(out)).stderr(Stdio::from(err));
            }
            _ => warn!("cannot hand the plug-in its log file; letting it inherit"),
        }
    }
    // A tray app owns no console, so a console child would be handed a
    // window of its own. See `poltertype_shell::configure_child`.
    poltertype_shell::configure_child(&mut cmd);
    cmd.spawn()
}

/// Open this service's log, truncating whatever the last run left, so
/// the file always answers "what happened this run" and cannot grow
/// without bound. Best-effort: a plug-in must still start where the log
/// directory cannot be created.
fn service_log(id: &str) -> Option<(PathBuf, std::fs::File)> {
    let dir = SettingsStore::log_dir().ok()?;
    if let Err(e) = std::fs::create_dir_all(&dir) {
        warn!(id = %id, "cannot create the log directory for the plug-in: {e}");
        return None;
    }
    // The id comes from a directory name on disk and could contain a
    // separator; keep it to one file in one place.
    let path = dir.join(format!("plugin-{}.log", id.replace(['/', '\\'], "-")));
    match std::fs::File::create(&path) {
        Ok(file) => Some((path, file)),
        Err(e) => {
            warn!(id = %id, path = ?path, "cannot open a log for the plug-in: {e}");
            None
        }
    }
}

/// The last non-blank line a plug-in wrote, trimmed to fit a
/// notification. Reads the end of the file only — a plug-in that logged
/// all day must not be pulled into memory to answer one question.
fn last_line(path: &std::path::Path) -> Option<String> {
    use std::io::{Read as _, Seek as _, SeekFrom};

    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let from = len.saturating_sub(LOG_TAIL_BYTES);
    file.seek(SeekFrom::Start(from)).ok()?;
    let mut buf = Vec::new();
    file.take(LOG_TAIL_BYTES).read_to_end(&mut buf).ok()?;

    let text = String::from_utf8_lossy(&buf);
    let line = text.lines().rev().find(|l| !l.trim().is_empty())?.trim();
    Some(match line.char_indices().nth(LOG_LINE_CHARS) {
        Some((cut, _)) => format!("{}…", &line[..cut]),
        None => line.to_owned(),
    })
}

/// How much of the end of a plug-in's log to read, and how much of the
/// line found there to repeat. Both are about a notification body, not
/// about diagnosis — the file itself is the diagnosis.
const LOG_TAIL_BYTES: u64 = 8 * 1024;
const LOG_LINE_CHARS: usize = 200;

#[cfg(test)]
#[path = "supervisor_tests.rs"]
mod tests;
