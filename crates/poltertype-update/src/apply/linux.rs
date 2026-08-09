//! Linux: replace the running AppImage with the downloaded one.

use std::path::PathBuf;

use tracing::info;

use super::{sh_quote, spawn_detached, write_script};
use crate::enums::UpdateError;
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

pub(super) fn apply(pending: &PendingUpdate, relaunch: bool) -> Result<(), UpdateError> {
    let target = running_appimage()?;
    let staging = crate::staging::staging_dir()?;

    // Overwrite the AppImage at the path it already lives at, keeping
    // its current file name even though the new build has a different
    // version in *its* name. The user's launcher entries, dock pins and
    // shell aliases all point at that path; a version-stamped rename
    // would silently break every one of them and leave the old binary
    // behind to be launched by mistake.
    let relaunch_line = if relaunch {
        format!("exec {} &\n", sh_quote(&target))
    } else {
        String::new()
    };

    let body = format!(
        "#!/bin/sh\n\
         # Written by PolterType {version} to install update {new_version}.\n\
         # Waits for the running app to exit, swaps the AppImage, relaunches.\n\
         set -e\n\
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
         mv -f {new} {target}\n\
         {relaunch_line}\
         rm -rf {staging}\n",
        version = crate::current_version(),
        new_version = pending.version,
        pid = std::process::id(),
        new = sh_quote(&pending.artifact),
        target = sh_quote(&target),
        staging = sh_quote(&staging),
    );

    let script = write_script("install.sh", &body)?;
    info!(?script, ?target, "spawning the AppImage swap");
    spawn_detached("sh", &[&script])
}
