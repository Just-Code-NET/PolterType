//! Handing a staged artifact to the OS installer.
//!
//! ## Why a script, and why it waits for us to die
//!
//! Every platform here has the same problem: the thing being replaced
//! is the thing doing the replacing. An MSI cannot overwrite a running
//! `poltertype.exe`; an AppImage cannot be `mv`-ed over while its own
//! FUSE mount is live. So none of these backends install anything
//! directly. Each writes a small script into the staging directory and
//! spawns it **detached**, and the script's first act is to wait for
//! our PID to disappear. The app then exits normally. The installer
//! runs in the gap, and relaunches us when it's done.
//!
//! Passing paths as script *files* rather than as command-line
//! arguments is deliberate. The paths involved are user home
//! directories, which routinely contain spaces, apostrophes and
//! non-ASCII — exactly the input that turns nested shell quoting into
//! a bug. A script on disk has one layer of quoting instead of three,
//! and it can be read afterwards by a user asking "what did it do to
//! my machine".
//!
//! Platform code lives here and in `poltertype-input` / `poltertype-layout`
//! and nowhere else — see `CLAUDE.md`.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use tracing::info;

use crate::enums::UpdateError;
use crate::staging;
use crate::types::PendingUpdate;

/// Install the staged update, then leave.
///
/// **The caller must exit promptly.** The spawned installer is blocked
/// on our process disappearing; if we linger, so does it. `relaunch`
/// says whether the installer should start PolterType again afterwards
/// — true when the user clicked "Restart to update", false when they
/// clicked Quit and asked for nothing more.
///
/// Returns `Ok(false)` when the update was discarded rather than
/// applied (too many failed attempts), so the caller can tell "we're
/// about to be replaced" from "carry on quitting".
pub fn apply(pending: &PendingUpdate, relaunch: bool) -> Result<bool, UpdateError> {
    if !staging::note_install_attempt(pending) {
        return Ok(false);
    }

    info!(
        version = %pending.version,
        artifact = ?pending.artifact,
        relaunch,
        "handing the staged update to the OS installer"
    );

    #[cfg(target_os = "linux")]
    linux::apply(pending, relaunch)?;
    #[cfg(target_os = "macos")]
    macos::apply(pending, relaunch)?;
    #[cfg(target_os = "windows")]
    windows::apply(pending, relaunch)?;

    Ok(true)
}

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

/// Spawn a process that outlives us.
///
/// Detachment is the whole point: this child has to still be alive
/// after the app it is replacing has exited. On Unix that means its own
/// process group, so a signal sent to ours (or a terminal hanging up on
/// a dev run) doesn't take the installer with it. On Windows it means
/// `DETACHED_PROCESS`, plus `CREATE_NO_WINDOW` so a console doesn't
/// flash up on a tray-only app.
///
/// All three stdio handles go to null. There is nobody to read them —
/// we're about to exit — and an installer blocking on a full pipe is a
/// hang with no user-visible cause.
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn spawn_detached(program: &str, args: &[&Path]) -> Result<(), UpdateError> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW);
    }

    cmd.spawn()?;
    Ok(())
}

/// Quote a path for a POSIX shell: wrap in single quotes, and end/
/// reopen the quoting around any literal `'`. Handles every byte a
/// path can contain, which `"$VAR"`-style interpolation does not.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn sh_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', r"'\''"))
}

/// Quote a path for PowerShell: single-quoted string, `'` doubled.
/// Inside single quotes PowerShell performs no expansion at all, so
/// `$`, backticks and `%` in a path are literals.
#[cfg(target_os = "windows")]
fn ps_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "''"))
}
