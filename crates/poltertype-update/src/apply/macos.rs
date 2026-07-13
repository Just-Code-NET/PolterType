//! macOS: mount the DMG, replace the .app bundle, relaunch.
//!
//! **Not validated on hardware.** macOS is a CI-only target for this
//! project (see `CLAUDE.md` § Known gaps), and this backend is written
//! from Apple's documentation. The Windows and Linux paths have been
//! exercised; this one has not.

use std::path::PathBuf;

use tracing::info;

use super::{sh_quote, spawn_detached, write_script};
use crate::enums::UpdateError;
use crate::types::PendingUpdate;

/// The `.app` bundle we are running out of.
///
/// `current_exe()` inside a bundle is
/// `…/PolterType.app/Contents/MacOS/poltertype`, so the bundle root is
/// three levels up. If those three levels don't spell `Contents/MacOS`
/// then we're a bare binary — someone's `cargo build` output, or a
/// Homebrew-style install — and we have no bundle to replace.
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

pub(super) fn apply(pending: &PendingUpdate, relaunch: bool) -> Result<(), UpdateError> {
    let bundle = running_bundle()?;
    let staging = crate::staging::staging_dir()?;

    let relaunch_line = if relaunch {
        format!("open {} || true\n", sh_quote(&bundle))
    } else {
        String::new()
    };

    // `ditto` rather than `cp -R`: it is Apple's own bundle-aware copy
    // and preserves resource forks, extended attributes and code-sign
    // metadata. `cp` mangles bundles in ways that only show up later,
    // as a launch failure with no useful error.
    //
    // The quarantine flag has to come off the *installed* copy. The DMG
    // was downloaded, so LaunchServices marked it quarantined, and that
    // flag is inherited by everything ditto'd out of it. Left in place,
    // it means the user gets "PolterType cannot be opened because the
    // developer cannot be verified" — for a build they already trusted
    // and are merely updating. We strip it for the same reason the
    // release notes tell first-time users to strip it by hand: the app
    // is unsigned (no Developer ID yet — see `docs/PLAN.md` Phase 9),
    // and Gatekeeper has nothing else to go on. Once we ship signed and
    // notarised builds, this line goes away.
    let body = format!(
        "#!/bin/sh\n\
         # Written by PolterType {version} to install update {new_version}.\n\
         set -e\n\
         \n\
         i=0\n\
         while kill -0 {pid} 2>/dev/null; do\n\
         \ti=$((i + 1))\n\
         \tif [ $i -gt 300 ]; then exit 1; fi\n\
         \tsleep 0.2\n\
         done\n\
         \n\
         MNT=$(mktemp -d /tmp/poltertype-update.XXXXXX)\n\
         hdiutil attach {dmg} -nobrowse -readonly -noverify -mountpoint \"$MNT\"\n\
         \n\
         # Stage the new bundle beside the old one, then swap. Copying\n\
         # straight over a live bundle would leave a half-replaced app\n\
         # if the copy failed midway; this way the destructive step is\n\
         # one rename.\n\
         NEW={bundle}.new\n\
         rm -rf \"$NEW\"\n\
         ditto \"$MNT\"/*.app \"$NEW\"\n\
         hdiutil detach \"$MNT\" -quiet || true\n\
         rmdir \"$MNT\" 2>/dev/null || true\n\
         \n\
         rm -rf {bundle}\n\
         mv \"$NEW\" {bundle}\n\
         xattr -dr com.apple.quarantine {bundle} || true\n\
         {relaunch_line}\
         rm -rf {staging}\n",
        version = crate::current_version(),
        new_version = pending.version,
        pid = std::process::id(),
        dmg = sh_quote(&pending.artifact),
        bundle = sh_quote(&bundle),
        staging = sh_quote(&staging),
    );

    let script = write_script("install.sh", &body)?;
    info!(?script, ?bundle, "spawning the .app bundle swap");
    spawn_detached("sh", &[&script])
}
