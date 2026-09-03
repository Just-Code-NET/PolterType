//! macOS: a per-user LaunchAgent, registered with `launchctl`.
//!
//! **This never calls `bootout`.** `launchctl bootout gui/<uid>/<label>`
//! terminates the job's running processes, and when launchd started us
//! at login *we are that job* — a single click could kill the app.
//! It reproduces only under launchd, never from Finder, which is why
//! hardware testing missed it; see docs/DECISIONS.md, 2026-07-30.
//!
//! So this writes and deletes the plist, and `bootstrap`s only when
//! launchd does not already know the label. One deliberate gap follows:
//! plist *contents* that drift while the label is loaded (an update
//! moved the executable) are corrected on disk immediately but not in
//! launchd until the next login.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use tracing::{debug, warn};

use crate::types::App;

fn plist_path(id: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join("Library/LaunchAgents")
            .join(format!("{id}.plist")),
    )
}

/// Escape the characters that would break out of an XML text node.
/// `&` and `<` are legal in macOS file names, so a user directory
/// called `Rock & Roll` must not produce a malformed plist.
pub(crate) fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub(crate) fn plist_body(id: &str, exe: &Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{program}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
"#,
        label = xml_escape(id),
        program = xml_escape(&exe.display().to_string()),
    )
}

/// Numeric uid for the `gui/<uid>` launchd domain. Shelling out to
/// `id -u` keeps this crate free of `unsafe` and of libc for one
/// integer.
fn uid() -> Option<String> {
    let out = Command::new("id").arg("-u").output().ok()?;
    let s = String::from_utf8(out.stdout).ok()?.trim().to_owned();
    (!s.is_empty()).then_some(s)
}

/// Does launchd already know this label in the user's GUI domain?
///
/// `launchctl print` exits non-zero for an unknown service, which is
/// all we need — its output format has changed between macOS releases
/// and is never parsed here.
fn is_registered(uid: &str, id: &str) -> bool {
    Command::new("launchctl")
        .args(["print", &format!("gui/{uid}/{id}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|st| st.success())
        .unwrap_or(false)
}

pub fn sync(enabled: bool, app: App<'_>) {
    let Some(path) = plist_path(app.id) else {
        warn!("could not resolve ~/Library/LaunchAgents; autostart unchanged");
        return;
    };

    if !enabled {
        if path.exists() {
            match std::fs::remove_file(&path) {
                // Deleting the plist is enough: launchd loads it only
                // at login, and a label already loaded this session has
                // RunAtLoad and no KeepAlive, so it starts nothing
                // again. Booting it out would kill us — see the module
                // docs.
                Ok(()) => debug!(?path, "autostart disabled: LaunchAgent removed"),
                Err(e) => warn!(?e, ?path, "could not remove LaunchAgent plist"),
            }
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
    let body = plist_body(app.id, &exe);

    if std::fs::read_to_string(&path).unwrap_or_default() != body {
        if let Some(dir) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(dir) {
                warn!(?e, ?dir, "could not create LaunchAgents dir");
                return;
            }
        }
        if let Err(e) = std::fs::write(&path, &body) {
            warn!(?e, ?path, "could not write LaunchAgent plist");
            return;
        }
        debug!(?path, "autostart enabled: LaunchAgent written");
    }

    // Register only when launchd has never heard of the label, so first
    // enable needs no relogin. Asking first also keeps us from touching
    // a job that might be this very process.
    let Some(uid) = uid() else {
        debug!("could not resolve uid; LaunchAgent will load at next login");
        return;
    };
    if is_registered(&uid, app.id) {
        return;
    }
    match Command::new("launchctl")
        .args(["bootstrap", &format!("gui/{uid}"), &path.to_string_lossy()])
        .status()
    {
        Ok(st) if st.success() => debug!(?path, "LaunchAgent bootstrapped"),
        Ok(st) => debug!(status = ?st, "launchctl bootstrap: non-zero exit"),
        Err(e) => warn!(?e, "could not run launchctl bootstrap"),
    }
}
