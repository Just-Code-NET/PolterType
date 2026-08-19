//! Windows: a value under the per-user run key.
//!
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` needs no
//! elevation and no installer cooperation: the value is ours, per user,
//! and Explorer runs it at sign-in.
//!
//! Driven through `reg.exe` rather than a registry binding — see
//! `lib.rs`. `CREATE_NO_WINDOW`, or a console flashes at every login.

use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use tracing::{debug, warn};

use crate::types::App;

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

/// `CREATE_NO_WINDOW` — keep `reg.exe` from flashing a console.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The run-key value data for `exe`.
///
/// The quotes are part of the data, not shell syntax: Windows parses
/// the stored string itself, and an unquoted `C:\Program Files\…`
/// would be tried as `C:\Program` first.
pub(crate) fn run_value(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

fn reg(args: &[&str]) -> Option<std::process::ExitStatus> {
    match Command::new("reg")
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status()
    {
        Ok(st) => Some(st),
        Err(e) => {
            warn!(?e, ?args, "could not run reg.exe");
            None
        }
    }
}

/// Read the current value data, if the value exists at all.
///
/// `reg query` writes a table; matching by substring against the exact
/// string we would set answers "is it already there?" without depending
/// on that table's layout.
fn value_matches(name: &str, want: &str) -> bool {
    let Ok(out) = Command::new("reg")
        .args(["query", RUN_KEY, "/v", name])
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    else {
        return false;
    };
    if !out.status.success() {
        return false;
    }
    String::from_utf8_lossy(&out.stdout).contains(want)
}

pub(crate) fn sync(enabled: bool, app: App<'_>) {
    if !enabled {
        // `/f` so reg.exe does not prompt; a missing value exits
        // non-zero, which is the state we wanted anyway.
        match reg(&["delete", RUN_KEY, "/v", app.id, "/f"]) {
            Some(st) if st.success() => debug!("autostart disabled: run-key value removed"),
            _ => debug!("autostart disabled: no run-key value to remove"),
        }
        return;
    }

    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            warn!(?e, "could not resolve own exe; autostart unchanged");
            return;
        }
    };
    let want = run_value(&exe);

    // Skip the write when it would change nothing — an update that
    // moved the executable is the case that must still go through.
    if value_matches(app.id, &want) {
        return;
    }
    match reg(&[
        "add", RUN_KEY, "/v", app.id, "/t", "REG_SZ", "/d", &want, "/f",
    ]) {
        Some(st) if st.success() => debug!(value = %want, "autostart enabled: run-key value set"),
        Some(st) => warn!(status = ?st, "reg.exe add failed; autostart unchanged"),
        None => {}
    }
}
