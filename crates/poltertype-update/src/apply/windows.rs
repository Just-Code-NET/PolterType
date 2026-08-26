//! Windows: run the MSI, then relaunch.

use std::path::Path;
#[cfg(target_os = "windows")]
use std::path::PathBuf;

#[cfg(target_os = "windows")]
use tracing::info;

use super::{HELLO, ps_quote};
#[cfg(target_os = "windows")]
use super::{spawn_detached, write_script};
use crate::consts::FAILED_FILE;
#[cfg(target_os = "windows")]
use crate::enums::UpdateError;
#[cfg(target_os = "windows")]
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

/// Verbose msiexec log, written beside the artifact. A refused install
/// leaves an exit code, which names the failure but never the cause;
/// this is the file that has the cause in it.
const MSI_LOG: &str = "msiexec.log";

/// Where the running `poltertype.exe` lives — what the installer
/// relaunches once the MSI has replaced it.
#[cfg(target_os = "windows")]
fn running_exe() -> Result<PathBuf, UpdateError> {
    Ok(std::env::current_exe()?)
}

#[cfg(target_os = "windows")]
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
    // Unconditional, and before the exit code is examined. The user
    // asked for PolterType back; a refused install leaves the old
    // binary in place and perfectly able to run, so the one outcome
    // worth ruling out is the machine ending up with no PolterType at
    // all. That is what used to happen, and what sent a user to the
    // release page for an MSI they already had installed.
    let relaunch_line = if relaunch {
        format!("Start-Process -FilePath {}\n", ps_quote(exe))
    } else {
        String::new()
    };

    format!(
        "# Written by PolterType {version} to install update {new_version}.\n\
         $ErrorActionPreference = 'Stop'\n\
         \n\
         # Everything this script prints is redirected to the app's log\n\
         # directory by the parent, so this first line is the proof that\n\
         # it ran at all — the distinction that hid a three-release bug\n\
         # in which PowerShell was spawned without a console and died\n\
         # before executing a single statement.\n\
         Write-Output \"{hello}, waiting for pid {pid}\"\n\
         \n\
         # Wait for the running app to exit — an MSI cannot replace a\n\
         # locked, in-use executable. Bounded so a wedged app can't\n\
         # leave this script resident forever.\n\
         Wait-Process -Id {pid} -Timeout 60 -ErrorAction SilentlyContinue\n\
         # …and give up rather than install over an app that is still\n\
         # there. The caller only quits once this script has said it is\n\
         # alive, so \"still running\" now means the hand-off was\n\
         # abandoned — and an MSI aimed at a live keyboard hook is the\n\
         # one thing this whole design exists to prevent.\n\
         if (Get-Process -Id {pid} -ErrorAction SilentlyContinue) {{\n\
         \tWrite-Output 'PolterType installer: the app is still running; nothing installed'\n\
         \texit 1\n\
         }}\n\
         \n\
         $msi = {msi}\n\
         $msilog = Join-Path {staging} '{msi_log}'\n\
         # {busy} is \"another installation is already in progress\": the\n\
         # machine hands out one MSI transaction at a time, so a support\n\
         # agent or Windows Update running at the wrong moment costs the\n\
         # user their update with nothing to show for it. Come back for\n\
         # it rather than treat contention as a failed package.\n\
         $code = {busy}\n\
         for ($i = 0; $i -lt {retries} -and $code -eq {busy}; $i++) {{\n\
         \tif ($i -gt 0) {{ Start-Sleep -Seconds {retry_secs} }}\n\
         \t$proc = Start-Process -FilePath 'msiexec.exe' \
         -ArgumentList '/i', \"`\"$msi`\"\", '/qb', '/norestart', '/l*v', \"`\"$msilog`\"\" \
         -Wait -PassThru\n\
         \t$code = $proc.ExitCode\n\
         \tWrite-Output \"PolterType installer: msiexec exit code $code\"\n\
         }}\n\
         \n\
         {relaunch_line}\
         \n\
         # Exit code 0 = installed, 3010 = installed, wants a reboot we\n\
         # asked it not to do. Anything else and we leave the staging\n\
         # directory alone: the artifact stays, the attempt counter in\n\
         # pending.json has already been bumped, and the app will retry\n\
         # on the next quit until it gives up on its own.\n\
         if ($code -eq 0 -or $code -eq 3010) {{\n\
         \tRemove-Item -LiteralPath {staging} -Recurse -Force -ErrorAction SilentlyContinue\n\
         }} else {{\n\
         \t# Read back by the app on its next start, which is how a\n\
         \t# refused install becomes a message instead of a restart that\n\
         \t# silently did nothing. $msilog says why.\n\
         \tSet-Content -LiteralPath (Join-Path {staging} '{failed}') \
         -Value \"msiexec exit code: $code\" -ErrorAction SilentlyContinue\n\
         }}\n",
        version = crate::current_version(),
        hello = HELLO,
        busy = MSI_BUSY,
        retries = BUSY_RETRIES,
        retry_secs = BUSY_RETRY_SECS,
        msi = ps_quote(artifact),
        staging = ps_quote(staging),
        msi_log = MSI_LOG,
        failed = FAILED_FILE,
    )
}

#[cfg(test)]
mod tests;
