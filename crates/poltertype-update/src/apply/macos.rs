//! macOS: mount the DMG, replace the .app bundle, relaunch.
//!
//! Validated on Apple Silicon at 0.19.0 (a 0.18.1 → 0.19.0 self-update
//! on an M1 Pro, reported in issue #3). Intel and the failure paths
//! below are still reasoned from Apple's documentation rather than run.

use std::path::Path;
#[cfg(target_os = "macos")]
use std::path::PathBuf;

#[cfg(target_os = "macos")]
use tracing::info;

use super::{HELLO, sh_quote};
#[cfg(target_os = "macos")]
use super::{spawn_detached, write_script};
use crate::consts::FAILED_FILE;
#[cfg(target_os = "macos")]
use crate::enums::UpdateError;
#[cfg(target_os = "macos")]
use crate::types::PendingUpdate;

/// The `.app` bundle we are running out of.
///
/// `current_exe()` inside a bundle is
/// `…/PolterType.app/Contents/MacOS/poltertype`, so the bundle root is
/// three levels up. If those three levels don't spell `Contents/MacOS`
/// then we're a bare binary — someone's `cargo build` output, or a
/// Homebrew-style install — and we have no bundle to replace.
#[cfg(target_os = "macos")]
fn running_bundle() -> Result<PathBuf, UpdateError> {
    let exe = std::env::current_exe()?;
    let bundle = exe
        .parent() // …/Contents/MacOS
        .filter(|p| p.file_name().is_some_and(|n| n == "MacOS"))
        .and_then(|p| p.parent()) // …/Contents
        .filter(|p| p.file_name().is_some_and(|n| n == "Contents"))
        .and_then(|p| p.parent()) // …/PolterType.app
        .filter(|p| p.extension().is_some_and(|e| e == "app"));

    bundle.map(PathBuf::from).ok_or_else(|| {
        UpdateError::UnsupportedInstall(format!(
            "not running from a .app bundle ({}) — install the DMG by hand",
            exe.display()
        ))
    })
}

#[cfg(target_os = "macos")]
pub(super) fn apply(pending: &PendingUpdate, relaunch: bool) -> Result<(), UpdateError> {
    let bundle = running_bundle()?;
    let staging = crate::staging::staging_dir()?;

    let body = script_body(
        &pending.version,
        &pending.artifact,
        &bundle,
        &staging,
        std::process::id(),
        relaunch,
    );

    let script = write_script("install.sh", &body)?;
    info!(?script, ?bundle, "spawning the .app bundle swap");
    spawn_detached("sh", &[&script])
}

/// The installer script, as text, so its shape can be asserted without
/// a Mac to run it on — which, for this backend, is the only way its
/// shape ever gets asserted at all.
fn script_body(
    new_version: &str,
    artifact: &Path,
    bundle: &Path,
    staging: &Path,
    pid: u32,
    relaunch: bool,
) -> String {
    // Unconditional, like the other two backends: an update that could
    // not be unpacked leaves the installed bundle exactly as it was, so
    // the user who asked for a restart still gets one.
    let relaunch_line = if relaunch {
        format!("open {} || true\n", sh_quote(bundle))
    } else {
        String::new()
    };

    // `ditto` rather than `cp -R`: it is Apple's own bundle-aware copy
    // and preserves resource forks, extended attributes and code-sign
    // metadata, which `cp` mangles in ways that surface later as a
    // launch failure with no useful error.
    //
    // The quarantine flag has to come off the *installed* copy: the DMG
    // was downloaded, so everything ditto'd out of it inherits the
    // flag, and the user would meet "cannot be opened because the
    // developer cannot be verified" for a build they already trusted.
    // Defensible only while the app is unsigned — this line goes away
    // the day we ship notarised builds.
    format!(
        "#!/bin/sh\n\
         # Written by PolterType {version} to install update {new_version}.\n\
         set -e\n\
         echo \"{hello}, waiting for pid {pid}\"\n\
         \n\
         i=0\n\
         while kill -0 {pid} 2>/dev/null; do\n\
         \ti=$((i + 1))\n\
         \tif [ $i -gt 300 ]; then exit 1; fi\n\
         \tsleep 0.2\n\
         done\n\
         \n\
         # Every step that talks to the disk image is tested rather than\n\
         # trusted: under `set -e` a refused mount or a short copy would\n\
         # abort the script, and the abort would take the relaunch with\n\
         # it — leaving a user who clicked \"Restart to update\" with no\n\
         # running app and no reason why.\n\
         ok=0\n\
         MNT=$(mktemp -d /tmp/poltertype-update.XXXXXX)\n\
         NEW={bundle}.new\n\
         rm -rf \"$NEW\"\n\
         if hdiutil attach {dmg} -nobrowse -readonly -noverify -mountpoint \"$MNT\"; then\n\
         \tif ditto \"$MNT\"/*.app \"$NEW\"; then ok=1; fi\n\
         \thdiutil detach \"$MNT\" -quiet || true\n\
         fi\n\
         rmdir \"$MNT\" 2>/dev/null || true\n\
         \n\
         # The installed bundle is moved aside rather than deleted, so a\n\
         # swap that fails half way can put back something that works\n\
         # instead of leaving an empty /Applications entry.\n\
         if [ \"$ok\" = 1 ]; then\n\
         \trm -rf {bundle}.old\n\
         \tmv {bundle} {bundle}.old\n\
         \tif mv \"$NEW\" {bundle}; then\n\
         \t\trm -rf {bundle}.old\n\
         \t\txattr -dr com.apple.quarantine {bundle} || true\n\
         \telse\n\
         \t\tmv {bundle}.old {bundle}\n\
         \t\tok=0\n\
         \tfi\n\
         else\n\
         \trm -rf \"$NEW\"\n\
         fi\n\
         \n\
         {relaunch_line}\
         if [ \"$ok\" = 1 ]; then\n\
         \trm -rf {staging}\n\
         else\n\
         \techo 'the DMG could not be unpacked over the installed bundle' > {failed}\n\
         fi\n",
        version = crate::current_version(),
        hello = HELLO,
        dmg = sh_quote(artifact),
        bundle = sh_quote(bundle),
        staging = sh_quote(staging),
        failed = sh_quote(&staging.join(FAILED_FILE)),
    )
}

#[cfg(test)]
mod tests;
