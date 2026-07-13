//! Windows: run the MSI, then relaunch.

use std::path::PathBuf;

use tracing::info;

use super::{ps_quote, spawn_detached, write_script};
use crate::enums::UpdateError;
use crate::types::PendingUpdate;

/// Where the running `poltertype.exe` lives — what the installer
/// relaunches once the MSI has replaced it.
fn running_exe() -> Result<PathBuf, UpdateError> {
    Ok(std::env::current_exe()?)
}

pub(super) fn apply(pending: &PendingUpdate, relaunch: bool) -> Result<(), UpdateError> {
    let exe = running_exe()?;
    let staging = crate::staging::staging_dir()?;

    // `/qb` (basic UI) rather than `/qn` (silent): the user asked for
    // this restart and a progress bar for the few seconds it takes is
    // reassurance, not noise. `/norestart` because an MSI has no
    // business rebooting a machine to update a tray app.
    //
    // Our MSI is a per-user install (see `installers/wix/main.wxs`), so
    // msiexec needs no elevation and the user gets no UAC prompt they
    // did not ask for. If that ever changes to a per-machine install,
    // this backend has to start asking for consent explicitly instead
    // of springing a UAC dialog on someone who clicked Quit.
    let relaunch_line = if relaunch {
        format!("Start-Process -FilePath {}\n", ps_quote(&exe))
    } else {
        String::new()
    };

    let body = format!(
        "# Written by PolterType {version} to install update {new_version}.\n\
         $ErrorActionPreference = 'Stop'\n\
         \n\
         # Wait for the running app to exit — an MSI cannot replace a\n\
         # locked, in-use executable. Bounded so a wedged app can't\n\
         # leave this script resident forever.\n\
         Wait-Process -Id {pid} -Timeout 60 -ErrorAction SilentlyContinue\n\
         \n\
         $msi = {msi}\n\
         $proc = Start-Process -FilePath 'msiexec.exe' \
         -ArgumentList '/i', \"`\"$msi`\"\", '/qb', '/norestart' -Wait -PassThru\n\
         \n\
         # Exit code 0 = installed, 3010 = installed, wants a reboot we\n\
         # asked it not to do. Anything else and we leave the staging\n\
         # directory alone: the artifact stays, the attempt counter in\n\
         # pending.json has already been bumped, and the app will retry\n\
         # on the next quit until it gives up on its own.\n\
         if ($proc.ExitCode -eq 0 -or $proc.ExitCode -eq 3010) {{\n\
         {relaunch_line}\
         \tRemove-Item -LiteralPath {staging} -Recurse -Force -ErrorAction SilentlyContinue\n\
         }}\n",
        version = crate::current_version(),
        new_version = pending.version,
        pid = std::process::id(),
        msi = ps_quote(&pending.artifact),
        staging = ps_quote(&staging),
    );

    // UTF-8 BOM: without it, `powershell.exe` (Windows PowerShell 5.1,
    // the one that is always present) decodes a .ps1 as the system ANSI
    // codepage, and any non-ASCII character in the user's home path —
    // which is exactly where the staging directory lives — turns into
    // mojibake and a "path not found" install failure.
    let mut bytes = String::from("\u{feff}");
    bytes.push_str(&body);
    let script = write_script("install.ps1", &bytes)?;

    info!(?script, ?exe, "spawning the MSI install");
    spawn_detached(
        "powershell.exe",
        &[
            std::path::Path::new("-NoProfile"),
            std::path::Path::new("-NonInteractive"),
            std::path::Path::new("-ExecutionPolicy"),
            std::path::Path::new("Bypass"),
            std::path::Path::new("-File"),
            &script,
        ],
    )
}
