//! Linux: replace the running AppImage with the downloaded one.

use std::path::Path;
#[cfg(target_os = "linux")]
use std::path::PathBuf;

#[cfg(target_os = "linux")]
use tracing::info;

use super::{HELLO, sh_quote};
#[cfg(target_os = "linux")]
use super::{spawn_detached, write_script};
use crate::consts::FAILED_FILE;
#[cfg(target_os = "linux")]
use crate::enums::UpdateError;
#[cfg(target_os = "linux")]
use crate::types::PendingUpdate;

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
pub(super) fn apply(pending: &PendingUpdate, relaunch: bool) -> Result<(), UpdateError> {
    let target = running_appimage()?;
    let staging = crate::staging::staging_dir()?;

    let body = script_body(
        &pending.version,
        &pending.artifact,
        &target,
        &staging,
        std::process::id(),
        relaunch,
    );

    let script = write_script("install.sh", &body)?;
    info!(?script, ?target, "spawning the AppImage swap");
    spawn_detached("sh", &[&script])
}

/// The installer script, as text, so its shape can be asserted without
/// replacing anybody's AppImage.
fn script_body(
    new_version: &str,
    artifact: &Path,
    target: &Path,
    staging: &Path,
    pid: u32,
    relaunch: bool,
) -> String {
    // Overwrite the AppImage at the path it already lives at, keeping
    // its current file name even though the new build has a different
    // version in *its* name. The user's launcher entries, dock pins and
    // shell aliases all point at that path; a version-stamped rename
    // would silently break every one of them and leave the old binary
    // behind to be launched by mistake.
    //
    // Unconditional, and outside the success test: a swap that failed
    // leaves the old AppImage in place and runnable, so the user still
    // gets the app they asked to have restarted.
    let relaunch_line = if relaunch {
        format!("{} &\n", sh_quote(target))
    } else {
        String::new()
    };

    format!(
        "#!/bin/sh\n\
         # Written by PolterType {version} to install update {new_version}.\n\
         # Waits for the running app to exit, swaps the AppImage, relaunches.\n\
         set -e\n\
         echo \"{hello}, waiting for pid {pid}\"\n\
         \n\
         # Poll rather than `wait`: the app is our parent, not our child,\n\
         # so there is nothing to wait(2) on. `kill -0` tests liveness\n\
         # without signalling. The bound stops a wedged app (one that\n\
         # ignores Quit) from leaving this script spinning forever.\n\
         i=0\n\
         while kill -0 {pid} 2>/dev/null; do\n\
         \ti=$((i + 1))\n\
         \tif [ $i -gt 300 ]; then exit 1; fi\n\
         \tsleep 0.2\n\
         done\n\
         \n\
         chmod +x {new}\n\
         # `mv` and not `cp`: on the same filesystem it is atomic, so a\n\
         # power cut can leave the old AppImage or the new one, never a\n\
         # half-written file that will not launch.\n\
         #\n\
         # Tested rather than trusted: this is the one step that can\n\
         # fail, and under `set -e` a failure here would take the\n\
         # relaunch down with it and leave the user with nothing running.\n\
         if mv -f {new} {target}; then\n\
         \tok=1\n\
         else\n\
         \tok=0\n\
         \techo 'PolterType installer: could not replace the AppImage'\n\
         fi\n\
         \n\
         {relaunch_line}\
         if [ \"$ok\" = 1 ]; then\n\
         \trm -rf {staging}\n\
         else\n\
         \techo 'could not replace the AppImage in place' > {failed}\n\
         fi\n",
        version = crate::current_version(),
        hello = HELLO,
        new = sh_quote(artifact),
        target = sh_quote(target),
        staging = sh_quote(staging),
        failed = sh_quote(&staging.join(FAILED_FILE)),
    )
}

#[cfg(test)]
mod tests;
