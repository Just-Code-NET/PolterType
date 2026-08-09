//! `run_shell` — running a program when a trigger fires.
//!
//! The one action that can do anything, so the one with a threat model
//! written down. PolterType already reads every keystroke; adding "and
//! can run a program" turns a stolen or mistaken `config.toml` into
//! code execution that fires the next time the user types an ordinary
//! word. Three routes in particular: a **synced config** pulled from
//! someone else's dotfiles repo; a **trigger that collides with prose**
//! (`date` is fine until someone writes "the release date is"); and
//! **shell metacharacters**, where every quoting bug in a string that
//! came from a config file is an injection.
//!
//! What answers each:
//!
//! * **Off unless switched on** — `[commands].allow_run_shell` is
//!   `false` by default, and entries on a machine that never enabled it
//!   run nothing and say so once per entry at load.
//! * **No shell.** [`ShellCommand`] is a program plus an argument
//!   vector, executed directly, so there is nothing for a
//!   metacharacter to mean. A user who wants a pipeline writes `sh` as
//!   the program and `-c` as an argument — explicit, and their call.
//! * **Nothing the user typed becomes an argument.** No placeholder
//!   substitutes the trigger or the buffer into the command line; that
//!   would smuggle arguments and put typed text into a process table
//!   other users can read.
//! * **Bounded** — a timeout, an output cap, no stdin.
//! * **Never on the typing path** — dispatch is fire-and-forget on a
//!   worker thread, so the word-boundary handler returns immediately.
//!
//! `insert_output = true` types the command's stdout at the cursor,
//! which is the point of the feature and its sharpest edge: whatever
//! the program prints goes into whatever window has focus. So output is
//! capped, trimmed to one logical line by default, and never inserted
//! when the command failed.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use tracing::{debug, warn};

/// Longest a triggered command may run before it is abandoned.
pub const RUN_TIMEOUT: Duration = Duration::from_secs(5);

/// Most stdout bytes kept when `insert_output` is set. A command that
/// prints a megabyte should not have a megabyte typed into the user's
/// editor one keystroke at a time.
pub const MAX_OUTPUT_BYTES: usize = 4 * 1024;

/// A program to run, already split — never a shell string.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ShellCommand {
    /// Executable to run. Resolved through `PATH` by the OS, as any
    /// other program launch is.
    pub program: String,
    /// Arguments, passed verbatim. No shell, no globbing, no
    /// substitution of anything the user typed.
    #[serde(default)]
    pub args: Vec<String>,
    /// Type the command's stdout at the cursor when it succeeds.
    #[serde(default)]
    pub insert_output: bool,
}

/// Why a `run_shell` entry will not run.
#[derive(Debug, PartialEq, Eq)]
pub enum ShellRefusal {
    /// `[commands].allow_run_shell` is false.
    NotEnabled,
    /// The entry names no program.
    EmptyProgram,
}

impl std::fmt::Display for ShellRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotEnabled => write!(
                f,
                "`run_shell` commands are disabled — set `[commands].allow_run_shell = true` \
                 to enable them, and read docs/SMART_COMMANDS.md first"
            ),
            Self::EmptyProgram => write!(f, "`run_shell` needs a non-empty `program`"),
        }
    }
}

/// Check an entry before running it. Separated from execution so the
/// settings loader can report a refusal once, at load, instead of
/// silently doing nothing every time the user types the trigger.
pub fn check(cmd: &ShellCommand, allow_run_shell: bool) -> Result<(), ShellRefusal> {
    if cmd.program.trim().is_empty() {
        return Err(ShellRefusal::EmptyProgram);
    }
    if !allow_run_shell {
        return Err(ShellRefusal::NotEnabled);
    }
    Ok(())
}

/// Run the command and return its stdout, if it should be inserted.
///
/// `None` whenever nothing should be typed: insertion off, the command
/// failed, it produced nothing, or it was abandoned. Never returns
/// `Err` — a failing user command belongs in the log, not threaded
/// through the engine.
///
/// **Must not be called from the word-boundary handler.** It blocks for
/// up to [`RUN_TIMEOUT`].
pub fn run(cmd: &ShellCommand) -> Option<String> {
    let started = Instant::now();
    let mut child = match Command::new(&cmd.program)
        .args(&cmd.args)
        // No stdin: a program that waits for input would otherwise
        // block until the timeout, every single time.
        .stdin(Stdio::null())
        .stdout(if cmd.insert_output {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            warn!(program = %cmd.program, %e, "smart command failed to start");
            return None;
        }
    };

    // Poll rather than `wait()`: a hung program must not keep this
    // worker thread forever.
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= RUN_TIMEOUT => {
                warn!(
                    program = %cmd.program,
                    timeout_s = RUN_TIMEOUT.as_secs(),
                    "smart command timed out; killing it"
                );
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => {
                warn!(program = %cmd.program, %e, "smart command wait failed");
                return None;
            }
        }
    };

    if !status.success() {
        warn!(
            program = %cmd.program,
            code = status.code().unwrap_or(-1),
            "smart command exited non-zero; nothing will be typed"
        );
        return None;
    }
    if !cmd.insert_output {
        debug!(program = %cmd.program, "smart command finished");
        return None;
    }

    let mut buf = Vec::new();
    if let Some(mut out) = child.stdout.take() {
        use std::io::Read;
        // Cap at the source: read one byte past the limit so an
        // over-long output is detected without buffering all of it.
        let mut limited = std::io::Read::take(&mut out, MAX_OUTPUT_BYTES as u64 + 1);
        if let Err(e) = limited.read_to_end(&mut buf) {
            warn!(program = %cmd.program, %e, "could not read smart command output");
            return None;
        }
    }
    Some(sanitise_output(&buf)).filter(|s| !s.is_empty())
}

/// Turn raw stdout into something safe to type — printing and typing
/// are not the same thing:
///
/// * **Truncate** to [`MAX_OUTPUT_BYTES`], on a character boundary.
/// * **Drop control characters.** A newline mid-output submits a chat
///   message or runs a shell line; escapes do stranger things.
///   Interior newlines become spaces, the rest of C0 goes.
/// * **Trim** the trailing newline the user never asked to have typed.
pub fn sanitise_output(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    let mut cut = text.len().min(MAX_OUTPUT_BYTES);
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    text[..cut]
        .trim()
        .chars()
        .map(|c| if c == '\n' || c == '\t' { ' ' } else { c })
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests;
