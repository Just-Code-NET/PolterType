//! Linux: replace the running AppImage with the downloaded one.
//!
//! The swap happens **in this process**, before anything is spawned.
//! An app that systemd started — which is what our own autostart unit
//! makes it — shares its cgroup with every helper it spawns, and the
//! default `KillMode=control-group` SIGKILLs whatever is left in that
//! cgroup the moment the app's main process exits. A helper waiting
//! for us to disappear is therefore killed at the exact instant it was
//! waiting for, mid-loop, before it reaches the swap. Detaching does
//! not help: a process group and a session are not a cgroup.
//!
//! So nothing that must not be lost is delegated to a process that has
//! to outlive us. What is left — starting the app again — is
//! best-effort and reported as such: an update that installed and did
//! not relaunch is not a failed update.

use std::path::Path;
#[cfg(target_os = "linux")]
use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::process::Command;

#[cfg(target_os = "linux")]
use tracing::{info, warn};

use super::{HELLO, sh_quote, sh_quote_str};
#[cfg(target_os = "linux")]
use super::{spawn_detached, write_script};
#[cfg(target_os = "linux")]
use crate::enums::{Applied, UpdateError};
#[cfg(target_os = "linux")]
use crate::types::PendingUpdate;

/// How many 200 ms polls the relaunch waits for the old process. Five
/// minutes: long enough that a slow shutdown is not mistaken for a
/// wedged one, short enough that a truly stuck process does not leave
/// a watcher on the machine for the rest of the session.
const WAIT_POLLS: u32 = 1500;

/// The transient unit the relaunch runs in under systemd. Named rather
/// than generated so that `journalctl --user -u` finds it.
#[cfg(target_os = "linux")]
const RELAUNCH_UNIT: &str = "poltertype-relaunch";

/// Path of the AppImage we are running from.
///
/// The AppImage runtime exports `$APPIMAGE`, the absolute path of the
/// `.AppImage` file itself — as opposed to `current_exe()`, which
/// points inside the temporary FUSE mount and is gone the moment we
/// exit. There is no other way to learn it.
///
/// Its absence is the honest signal that this build did not come from
/// an AppImage: a dev binary, a distro package, or a bare binary in
/// `~/.local/bin`. None of those are ours to overwrite — replacing a
/// packaged file behind the package manager's back is how you corrupt a
/// system — so we refuse and the caller points the user at the release
/// page instead.
#[cfg(target_os = "linux")]
fn running_appimage() -> Result<PathBuf, UpdateError> {
    match std::env::var_os("APPIMAGE") {
        Some(p) if !p.is_empty() => Ok(PathBuf::from(p)),
        _ => Err(UpdateError::UnsupportedInstall(
            "not running from an AppImage ($APPIMAGE is unset) — \
             a package-manager or development build must be updated the way it was installed"
                .to_owned(),
        )),
    }
}

#[cfg(target_os = "linux")]
pub(super) fn apply(pending: &PendingUpdate, relaunch: bool) -> Result<Applied, UpdateError> {
    let target = running_appimage()?;

    if let Err(e) = swap_in_place(&pending.artifact, &target) {
        // A swap that failed has used up this artifact's turn. Without
        // the count the ceiling never moves, and a file this machine
        // can never install would be retried on every quit, forever.
        crate::staging::note_install_attempt(pending);
        return Err(e);
    }
    info!(version = %pending.version, ?target, "AppImage replaced in place");

    if !relaunch {
        return Ok(Applied::HandedOff);
    }
    match arrange_relaunch(&target) {
        Ok(()) => Ok(Applied::HandedOff),
        Err(e) => {
            warn!(
                ?e,
                "the update is installed, but nothing here can start PolterType again"
            );
            Ok(Applied::InstalledStayUp)
        }
    }
}

/// Put the staged AppImage where the running one lives.
///
/// `rename` over the *running* AppImage is safe: it replaces a
/// directory entry, and the image we execute from keeps the old inode
/// alive through its own open descriptor until we exit. It is also
/// atomic, so an interrupted swap leaves the old AppImage or the new
/// one, never a half-written file that will not launch.
///
/// The target keeps its current file name even though the new build
/// has a different version in *its* name: launcher entries, systemd
/// units, dock pins and shell aliases all point at that path, and a
/// version-stamped rename would break every one of them and leave the
/// old binary behind to be started by mistake.
#[cfg(target_os = "linux")]
fn swap_in_place(artifact: &Path, target: &Path) -> Result<(), UpdateError> {
    use std::os::unix::fs::PermissionsExt;

    // The download lands 0644, and an AppImage nobody can execute is
    // not an installed app.
    std::fs::set_permissions(artifact, std::fs::Permissions::from_mode(0o755))?;

    if std::fs::rename(artifact, target).is_ok() {
        return Ok(());
    }

    // Staging and the installed AppImage need not share a filesystem.
    // Copy to a sibling of the target first, so the step that replaces
    // it is still the atomic rename above.
    let temp = target
        .parent()
        .unwrap_or(Path::new("."))
        .join(format!(".poltertype-update-{}", std::process::id()));
    std::fs::copy(artifact, &temp)?;
    std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o755))?;
    if let Err(e) = std::fs::rename(&temp, target) {
        let _ = std::fs::remove_file(&temp);
        return Err(e.into());
    }
    let _ = std::fs::remove_file(artifact);
    Ok(())
}

/// Get PolterType started again once this process is gone.
///
/// Two shapes, because what can outlive us differs:
///
/// * Inside a systemd **service**, nothing we spawn can: the cgroup is
///   emptied when we exit. `systemd-run` hands the wait to a transient
///   unit of its own — a separate cgroup, which our teardown cannot
///   reach — and that unit starts the service back up, which is also
///   the only way to end up running *inside* the unit again rather
///   than beside a dead one.
/// * Anywhere else — a `.scope` from a desktop launcher or a terminal,
///   a session with no user manager at all — a detached child does
///   survive, because a scope has no main process whose exit stops it.
///   A script that waits and execs the AppImage is simpler there and
///   assumes no systemd.
#[cfg(target_os = "linux")]
fn arrange_relaunch(target: &Path) -> Result<(), UpdateError> {
    let unit = std::fs::read_to_string("/proc/self/cgroup")
        .ok()
        .and_then(|c| service_from_cgroup(&c).map(str::to_owned));

    let body = script_body(std::process::id(), &launch_line(unit.as_deref(), target));
    let script = write_script("relaunch.sh", &body)?;

    let Some(unit) = unit else {
        info!(?script, ?target, "spawning the relaunch watcher");
        return spawn_detached("sh", &[&script]);
    };

    // The transient unit's own output goes to the journal, not to
    // `installer.log`: name both here so whoever reads this line knows
    // where the other half is.
    info!(
        %unit,
        relaunch_unit = RELAUNCH_UNIT,
        ?script,
        "handing the relaunch to a transient systemd unit"
    );
    let out = Command::new("systemd-run")
        .args([
            "--user",
            "--collect",
            "--quiet",
            "--unit",
            RELAUNCH_UNIT,
            "--",
            "sh",
        ])
        .arg(&script)
        .output()?;
    if out.status.success() {
        return Ok(());
    }
    Err(UpdateError::RelaunchFailed(format!(
        "systemd-run {}: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr).trim()
    )))
}

/// The systemd user unit this process belongs to, if it is a service.
///
/// A `.scope` is deliberately not one: a scope stops when its last
/// process exits rather than when a main process does, so a helper
/// left behind in it is not killed and the plain script is the simpler
/// answer there.
fn service_from_cgroup(cgroup: &str) -> Option<&str> {
    let path = cgroup.lines().find_map(|l| l.strip_prefix("0::"))?;
    let unit = path.rsplit('/').next()?.trim();
    (unit.ends_with(".service") && !unit.contains(char::is_whitespace)).then_some(unit)
}

/// The tail of the relaunch script: how the app is started again.
///
/// Under a service the unit is started rather than the file, so the
/// app comes back *inside* the unit; the file is the fallback for the
/// unit that cannot be started twice — a transient one, or one whose
/// definition went away while we were running.
fn launch_line(unit: Option<&str>, target: &Path) -> String {
    let direct = format!("exec {}", sh_quote(target));
    match unit {
        Some(unit) => format!(
            "systemctl --user start {unit} && exit 0\n{direct}",
            unit = sh_quote_str(unit),
        ),
        None => direct,
    }
}

/// The relaunch script, as text, so its shape can be asserted without
/// restarting anybody's desktop.
fn script_body(pid: u32, launch: &str) -> String {
    format!(
        "#!/bin/sh\n\
         # Written by PolterType {version}. The update is already\n\
         # installed; this only starts the app again once the old\n\
         # process is gone.\n\
         echo \"{hello}, waiting for pid {pid}\"\n\
         \n\
         # Poll rather than `wait`: the app is not our child, so there\n\
         # is nothing to wait(2) on. `kill -0` tests liveness without\n\
         # signalling. The bound stops a wedged app from leaving a\n\
         # watcher spinning for the rest of the session.\n\
         i=0\n\
         while kill -0 {pid} 2>/dev/null; do\n\
         \ti=$((i + 1))\n\
         \tif [ $i -gt {bound} ]; then\n\
         \t\techo 'PolterType installer: the old process never exited; not starting a second one'\n\
         \t\texit 1\n\
         \tfi\n\
         \tsleep 0.2\n\
         done\n\
         \n\
         {launch}\n",
        version = crate::current_version(),
        hello = HELLO,
        bound = WAIT_POLLS,
    )
}

#[cfg(test)]
mod tests;
