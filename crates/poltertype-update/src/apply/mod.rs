//! Handing a staged artifact to the OS installer.
//!
//! Windows and macOS have the same problem: the thing being replaced
//! is the thing doing the replacing. An MSI cannot overwrite a running
//! `poltertype.exe`. So neither backend installs anything directly —
//! each writes a small script into the staging directory and spawns
//! it, and the script's first act is to wait for our PID to disappear.
//! The installer runs in the gap and relaunches us after.
//!
//! Linux does not, and must not: a helper spawned by an app systemd
//! started is killed the moment that app exits, which is the moment it
//! was waiting for. It swaps the AppImage in-process instead — see
//! [`linux`], which carries the whole story.
//!
//! Paths go into the script as *files* rather than command-line
//! arguments: they are user home directories, which routinely contain
//! spaces, apostrophes and non-ASCII — exactly the input that turns
//! nested shell quoting into a bug. A script on disk has one layer of
//! quoting instead of three, and can be read afterwards by a user
//! asking what it did to their machine.

// All three are compiled when testing, whatever the host is: the
// installer script is text, and text is the only part of an installer
// that can be checked without installing something. Their assertions
// used to run only on the platform they install for — a poor place to
// keep the tests for a bug that shipped three times, and for a macOS
// backend nobody in the project can run at all.
#[cfg(any(target_os = "linux", test))]
mod linux;
#[cfg(any(target_os = "macos", test))]
mod macos;
#[cfg(any(target_os = "windows", test))]
mod windows;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use tracing::{info, warn};

use crate::enums::{Applied, UpdateError};
use crate::staging;
use crate::types::PendingUpdate;

/// Install the staged update, then leave.
///
/// **On [`Applied::HandedOff`] the caller must exit promptly** — an
/// installer may be blocked on our process disappearing. `relaunch`
/// says whether PolterType should be started again afterwards; the
/// other two outcomes both mean *keep running*, and are the difference
/// between an update that is not coming and one that has arrived but
/// cannot restart us.
///
/// `macos_sign_identity` is `[updates].local_signing_identity`: on
/// macOS the installer re-signs the swapped bundle with it (TCC grants
/// then survive the update), or, when empty, resets the two stale TCC
/// records so the Setup pane can re-ask cleanly. The other platforms
/// ignore it.
pub fn apply(
    pending: &PendingUpdate,
    relaunch: bool,
    macos_sign_identity: &str,
) -> Result<Applied, UpdateError> {
    let _ = macos_sign_identity;
    if !staging::attempts_left(pending) {
        return Ok(Applied::Discarded);
    }

    info!(
        version = %pending.version,
        artifact = ?pending.artifact,
        relaunch,
        "handing the staged update to the OS installer"
    );

    #[cfg(target_os = "linux")]
    let applied = linux::apply(pending, relaunch)?;

    // Linux is not here because its swap has already succeeded or
    // already failed by the time it returns, and it counts its own
    // failures. Everywhere else the outcome is still a spawned
    // process's to decide.
    #[cfg(not(target_os = "linux"))]
    let applied = {
        #[cfg(target_os = "macos")]
        macos::apply(pending, relaunch, macos_sign_identity)?;
        #[cfg(target_os = "windows")]
        windows::apply(pending, relaunch)?;

        // Only now: the installer has spoken, so this artifact really
        // did get a turn. Counting a spawn that never ran is what
        // threw away three verified downloads on a machine where the
        // installer could not start at all.
        staging::note_install_attempt(pending);
        Applied::HandedOff
    };

    Ok(applied)
}

/// What every installer script prints before it does anything that can
/// fail. Read back by [`await_greeting`].
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
const HELLO: &str = "PolterType installer: started";

/// How long to give the installer to say it is alive. Generous: a cold
/// `powershell.exe` on a machine with an eager antivirus can take a
/// second or two to reach its first statement, and a false negative
/// here costs the user an update they asked for.
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
const GREETING_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
const GREETING_POLL: Duration = Duration::from_millis(50);

/// Write an installer script next to the artifact it installs.
///
/// Lives in the staging directory so that the successful path cleans
/// itself up: the last thing every script does is delete the directory
/// it is running from, script included.
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn write_script(name: &str, body: &str) -> Result<PathBuf, UpdateError> {
    let dir = staging::staging_dir()?;
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(name);
    std::fs::write(&path, body)?;
    Ok(path)
}

/// Spawn the installer so it outlives us, with its output on record.
///
/// The child must still be alive after the app it is replacing has
/// exited. On Unix that means its own process group, so a signal sent to
/// ours does not take the installer with it.
///
/// **That is not enough under systemd, and Linux no longer relies on
/// it for anything that matters.** A process group is not a cgroup: a
/// `.service` is stopped when its main process exits and takes the
/// whole cgroup down with it, helper included. See [`linux`].
///
/// **Windows takes only `CREATE_NO_WINDOW`, and this is load-bearing.**
/// It used to take `DETACHED_PROCESS` as well, on the reasoning that
/// detachment is what keeps a child alive — which is not true on
/// Windows, where a child is never killed by its parent exiting.
/// `DETACHED_PROCESS` means something else: the process gets *no
/// console at all*, and Windows PowerShell 5.1's console host cannot
/// start without one. Every self-update this app ever attempted on
/// Windows died there — the event log records `powershell.exe` logging
/// "console is starting up" and then nothing, five times over three
/// releases, with not one MSI transaction to show for it.
/// `CREATE_NO_WINDOW` gives the child its own console and no window,
/// which is what a tray app wanted in the first place.
///
/// stdout and stderr go to a log file rather than to null: an installer
/// that never ran and one that ran and failed used to look identical
/// from the outside, which is how this bug survived three releases.
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn spawn_detached(program: &str, args: &[&Path]) -> Result<(), UpdateError> {
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::null());

    let watch = match installer_log() {
        Ok((path, out, err)) => {
            cmd.stdout(out).stderr(err);
            Some(path)
        }
        Err(e) => {
            // A missing log must not cost the user their update: with
            // nowhere to read a greeting from we go back to trusting
            // the spawn, which is what every version before this did.
            warn!(?e, "installer output will not be recorded");
            cmd.stdout(Stdio::null()).stderr(Stdio::null());
            None
        }
    };

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    cmd.spawn()?;

    match watch {
        Some(path) if !await_greeting(&path) => Err(UpdateError::InstallerSilent(format!(
            "{program} was started but never reached its first line; see {}",
            path.display()
        ))),
        _ => Ok(()),
    }
}

/// Both output streams of the installer, pointed at
/// [`staging::installer_log_path`], plus the path to read them back.
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn installer_log() -> Result<(PathBuf, Stdio, Stdio), UpdateError> {
    let path = staging::installer_log_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::File::create(&path)?;
    let dup = file.try_clone()?;
    Ok((path, Stdio::from(file), Stdio::from(dup)))
}

/// Wait for the installer to announce itself.
///
/// A successful `spawn` only proves the OS created a process. It says
/// nothing about whether that process ran: `powershell.exe` given no
/// console is created, records that it is starting, and dies before its
/// first statement — and from in here that was indistinguishable from a
/// hand-off that worked, which is how a broken Windows updater survived
/// three releases. So every script says one line first and we read it
/// back before letting the caller quit for an installer that is not
/// there.
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn await_greeting(path: &Path) -> bool {
    let deadline = Instant::now() + GREETING_TIMEOUT;
    loop {
        if std::fs::read_to_string(path).is_ok_and(|s| s.contains(HELLO)) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(GREETING_POLL);
    }
}

/// `sh -n`, shared by the two POSIX backends' tests.
///
/// Parsing without executing is as far as a script that replaces the
/// user's installed application can be exercised here — but a syntax
/// error in one is not something to find out about on somebody's
/// machine, and until now nothing checked even that.
#[cfg(all(test, unix))]
pub(crate) mod tests_util {
    use std::io::Write;
    use std::process::{Command, Stdio};

    pub(crate) fn assert_sh_parses(body: &str) {
        let mut sh = Command::new("sh")
            .arg("-n")
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn sh");
        sh.stdin
            .take()
            .expect("stdin")
            .write_all(body.as_bytes())
            .expect("write script");
        let out = sh.wait_with_output().expect("wait");
        assert!(
            out.status.success(),
            "sh refused the script: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

/// Quote a string for a POSIX shell: wrap in single quotes, and end/
/// reopen the quoting around any literal `'`. Handles every byte a
/// path or a unit name can contain, which `"$VAR"`-style interpolation
/// does not.
#[cfg(any(target_os = "linux", target_os = "macos", test))]
fn sh_quote_str(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[cfg(any(target_os = "linux", target_os = "macos", test))]
fn sh_quote(path: &Path) -> String {
    sh_quote_str(&path.to_string_lossy())
}

/// Quote a path for PowerShell: single-quoted string, `'` doubled.
/// Inside single quotes PowerShell performs no expansion at all, so
/// `$`, backticks and `%` in a path are literals.
#[cfg(any(target_os = "windows", test))]
fn ps_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "''"))
}
