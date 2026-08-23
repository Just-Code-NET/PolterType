//! Windows: run the MSI, then relaunch.

use std::path::{Path, PathBuf};

use tracing::info;

use super::{ps_quote, spawn_detached, write_script};
use crate::enums::UpdateError;
use crate::types::PendingUpdate;

/// `ERROR_INSTALL_ALREADY_RUNNING`. Windows serialises MSI
/// transactions machine-wide, so a vendor's support agent or Windows
/// Update holding the installer mutex makes ours fail instantly with
/// this — contention, not a broken package.
const MSI_BUSY: u32 = 1618;
/// How many times to come back for the mutex, and how long to wait
/// between tries: five minutes in total, which outlasts the small
/// transactions that cause this and still ends.
const BUSY_RETRIES: u32 = 20;
const BUSY_RETRY_SECS: u32 = 15;

/// Where the running `poltertype.exe` lives — what the installer
/// relaunches once the MSI has replaced it.
fn running_exe() -> Result<PathBuf, UpdateError> {
    Ok(std::env::current_exe()?)
}

pub(super) fn apply(pending: &PendingUpdate, relaunch: bool) -> Result<(), UpdateError> {
    let exe = running_exe()?;
    let staging = crate::staging::staging_dir()?;

    let body = script_body(
        &pending.version.to_string(),
        &pending.artifact,
        &exe,
        &staging,
        std::process::id(),
        relaunch,
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
            Path::new("-NoProfile"),
            Path::new("-NonInteractive"),
            Path::new("-ExecutionPolicy"),
            Path::new("Bypass"),
            Path::new("-File"),
            &script,
        ],
    )
}

/// The installer script, as text, so its shape can be asserted without
/// running an installer.
///
/// `/qb` rather than `/qn`: the user asked for this restart, and a
/// progress bar for the few seconds it takes is reassurance.
/// `/norestart` because an MSI has no business rebooting a machine to
/// update a tray app.
///
/// Our MSI is a per-user install, so msiexec needs no elevation. If
/// that ever becomes per-machine, this backend must ask for consent
/// rather than spring a UAC dialog on someone who clicked Quit.
fn script_body(
    new_version: &str,
    artifact: &Path,
    exe: &Path,
    staging: &Path,
    pid: u32,
    relaunch: bool,
) -> String {
    let relaunch_line = if relaunch {
        format!("\tStart-Process -FilePath {}\n", ps_quote(exe))
    } else {
        String::new()
    };

    format!(
        "# Written by PolterType {version} to install update {new_version}.\n\
         $ErrorActionPreference = 'Stop'\n\
         \n\
         # Wait for the running app to exit — an MSI cannot replace a\n\
         # locked, in-use executable. Bounded so a wedged app can't\n\
         # leave this script resident forever.\n\
         Wait-Process -Id {pid} -Timeout 60 -ErrorAction SilentlyContinue\n\
         \n\
         $msi = {msi}\n\
         # {busy} is \"another installation is already in progress\": the\n\
         # machine hands out one MSI transaction at a time, so a support\n\
         # agent or Windows Update running at the wrong moment costs the\n\
         # user their update with nothing to show for it. Come back for\n\
         # it rather than treat contention as a failed package.\n\
         $code = {busy}\n\
         for ($i = 0; $i -lt {retries} -and $code -eq {busy}; $i++) {{\n\
         \tif ($i -gt 0) {{ Start-Sleep -Seconds {retry_secs} }}\n\
         \t$proc = Start-Process -FilePath 'msiexec.exe' \
         -ArgumentList '/i', \"`\"$msi`\"\", '/qb', '/norestart' -Wait -PassThru\n\
         \t$code = $proc.ExitCode\n\
         }}\n\
         \n\
         # Exit code 0 = installed, 3010 = installed, wants a reboot we\n\
         # asked it not to do. Anything else and we leave the staging\n\
         # directory alone: the artifact stays, the attempt counter in\n\
         # pending.json has already been bumped, and the app will retry\n\
         # on the next quit until it gives up on its own.\n\
         if ($code -eq 0 -or $code -eq 3010) {{\n\
         {relaunch_line}\
         \tRemove-Item -LiteralPath {staging} -Recurse -Force -ErrorAction SilentlyContinue\n\
         }} else {{\n\
         \t# The one thing a user can find afterwards. Nothing reads it\n\
         \t# back: without it a refused install is a restart that simply\n\
         \t# did nothing, with no exit code left anywhere on the machine.\n\
         \tSet-Content -LiteralPath (Join-Path {staging} 'install-failed.txt') \
         -Value \"msiexec exit code: $code\" -ErrorAction SilentlyContinue\n\
         }}\n",
        version = crate::current_version(),
        busy = MSI_BUSY,
        retries = BUSY_RETRIES,
        retry_secs = BUSY_RETRY_SECS,
        msi = ps_quote(artifact),
        staging = ps_quote(staging),
    )
}

#[cfg(test)]
mod tests;
