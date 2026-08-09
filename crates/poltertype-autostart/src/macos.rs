//! macOS: a per-user LaunchAgent, registered with `launchctl`.
//!
//! **This never calls `bootout`.** `launchctl bootout gui/<uid>/<label>`
//! does not merely forget a job spec — it terminates the job's running
//! processes, and when launchd started us at login *we are that job*.
//! The first draft did it on both paths, and both were reachable from a
//! single click: enable/startup booted us out and bootstrapped, so
//! launchd killed us and the replacement hit our own still-held
//! instance lock and exited, leaving nothing running; and disable
//! terminated the app on the spot when the user unticked the box.
//!
//! Neither shows up when the app is launched from Finder, because then
//! it is not a launchd job and bootout has no process to kill — which
//! is why hardware testing missed it.
//!
//! So this writes and deletes the plist, and `bootstrap`s only when
//! launchd does not already know the label. The cost is one deliberate
//! gap: if the plist *contents* drift while the label is loaded — an
//! update moved the executable — launchd keeps the old spec until the
//! next login. The file on disk is corrected immediately, so the next
//! login is right.

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
/// `&` and `<` are the ones that matter; both are legal in macOS file
/// names, so a user directory called `Rock & Roll` must not produce a
/// malformed plist. `>` needs no escaping in a text node but is
/// conventional and harmless.
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
/// `id -u` keeps this crate free of `unsafe` and of a libc dependency
/// for one integer.
fn uid() -> Option<String> {
    let out = Command::new("id").arg("-u").output().ok()?;
    let s = String::from_utf8(out.stdout).ok()?.trim().to_owned();
    (!s.is_empty()).then_some(s)
}

/// Does launchd already know this label in the user's GUI domain?
///
/// `launchctl print` exits non-zero for an unknown service, which is
/// all we need — we never parse its output, whose format Apple has
/// changed between releases.
fn is_registered(uid: &str, id: &str) -> bool {
    Command::new("launchctl")
        .args(["print", &format!("gui/{uid}/{id}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|st| st.success())
        .unwrap_or(false)
}

pub(crate) fn sync(enabled: bool, app: App<'_>) {
    let Some(path) = plist_path(app.id) else {
        warn!("could not resolve ~/Library/LaunchAgents; autostart unchanged");
        return;
    };

    if !enabled {
        if path.exists() {
            match std::fs::remove_file(&path) {
                // Deleting the plist is enough: launchd only loads it
                // at login, and the label already loaded in this
                // session has RunAtLoad behind it and no KeepAlive,
                // so it will never start anything again. See the
                // module note on why we do not boot it out.
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

    // Register only when launchd has never heard of the label, so
    // coverage starts without a relogin on first enable. Bootstrapping
    // an already-known label would error anyway; asking first also
    // keeps us from touching a job that might be this very process.
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
