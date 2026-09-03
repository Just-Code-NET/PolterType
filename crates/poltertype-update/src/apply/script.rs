//! Writing an installer script to disk, spawning it detached, and
//! waiting for its first line before letting the caller quit.
//!
//! stdout and stderr go to a log file rather than to null: an
//! installer that never ran and one that ran and failed used to look
//! identical from the outside, which is how a Windows self-update bug
//! survived three releases — see `docs/DECISIONS.md`, 2026-08-26.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use tracing::warn;

use crate::enums::UpdateError;
use crate::staging;

use super::consts::{GREETING_POLL, GREETING_TIMEOUT, HELLO};
#[cfg(unix)]
use super::unix::detach;
#[cfg(windows)]
use super::windows::detach;

/// Write an installer script next to the artifact it installs.
///
/// Lives in the staging directory so that the successful path cleans
/// itself up: the last thing every script does is delete the directory
/// it is running from, script included.
pub(super) fn write_script(name: &str, body: &str) -> Result<PathBuf, UpdateError> {
    let dir = staging::staging_dir()?;
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(name);
    std::fs::write(&path, body)?;
    Ok(path)
}

/// Spawn the installer so it outlives us, with its output on record.
///
/// A successful `spawn` only proves the OS created a process, not that
/// it ran — see `docs/DECISIONS.md`, 2026-08-26. So every script says
/// one line first, and this reads it back before letting the caller
/// quit for an installer that is not there.
pub(super) fn spawn_detached(program: &str, args: &[&Path]) -> Result<(), UpdateError> {
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

    detach(&mut cmd);
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
fn installer_log() -> Result<(PathBuf, Stdio, Stdio), UpdateError> {
    let path = staging::installer_log_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::File::create(&path)?;
    let dup = file.try_clone()?;
    Ok((path, Stdio::from(file), Stdio::from(dup)))
}

/// Poll for the installer's greeting until it appears or the deadline
/// passes.
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
