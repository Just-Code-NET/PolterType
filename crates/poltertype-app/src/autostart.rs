//! Apply `[general].autostart` to the OS autostart mechanism.
//!
//! The setting existed long before any platform honoured it — the
//! Settings GUI checkbox only edited `config.toml`. This module is
//! where the checkbox becomes real.
//!
//! * **macOS** — a per-user LaunchAgent at
//!   `~/Library/LaunchAgents/<APP_ID>.plist`
//!   (`dev.opensource.poltertype.plist`). We rewrite it when the exe
//!   path changed (an update replaced / moved the .app), register it
//!   with launchd immediately so coverage starts without a relogin,
//!   and delete it when the toggle is off. Modern macOS shows a
//!   one-time "login item added" notification for this; that is the
//!   system working as intended.
//! * **Windows / Linux** — still unimplemented (same as before):
//!   the sync is a no-op there.

#[cfg(target_os = "macos")]
mod imp {
    use std::path::{Path, PathBuf};

    use tracing::{debug, warn};

    use crate::consts::APP_ID;

    fn plist_path() -> Option<PathBuf> {
        let home = std::env::var_os("HOME")?;
        Some(
            PathBuf::from(home)
                .join("Library/LaunchAgents")
                .join(format!("{APP_ID}.plist")),
        )
    }

    /// Minimal XML escaping for the program path (`&` is legal in
    /// file names).
    fn xml_escape(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    }

    fn plist_body(exe: &Path) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{APP_ID}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
"#,
            xml_escape(&exe.display().to_string())
        )
    }

    fn launchctl(args: &[&str]) {
        match std::process::Command::new("launchctl").args(args).status() {
            Ok(st) if !st.success() => {
                debug!(?args, status = ?st, "launchctl: non-zero exit (fine if job was (un)loaded already)")
            }
            Ok(_) => {}
            Err(e) => warn!(?e, ?args, "could not run launchctl"),
        }
    }

    /// Numeric uid for the `gui/<uid>` launchd domain, without
    /// touching libc (the crate forbids unsafe code).
    fn uid() -> Option<String> {
        let out = std::process::Command::new("id").arg("-u").output().ok()?;
        let s = String::from_utf8(out.stdout).ok()?.trim().to_owned();
        (!s.is_empty()).then_some(s)
    }

    pub fn sync(enabled: bool) {
        let Some(path) = plist_path() else {
            warn!("could not resolve ~/Library/LaunchAgents; autostart unchanged");
            return;
        };

        if !enabled {
            if path.exists() {
                // Unregister the running job first; bootout on an
                // already-absent job errors, which is fine.
                if let Some(uid) = uid() {
                    launchctl(&["bootout", &format!("gui/{uid}/{APP_ID}")]);
                }
                if let Err(e) = std::fs::remove_file(&path) {
                    warn!(?e, ?path, "could not remove LaunchAgent plist");
                } else {
                    debug!(?path, "autostart disabled: LaunchAgent removed");
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
        let body = plist_body(&exe);

        // Rewrite the file when its contents drifted (the exe path
        // moved under an update, an older build wrote a different
        // label, …). When it's already byte-identical we skip the
        // write — but NOT the launchd registration below: a plist can
        // be on disk yet unregistered (written by an older build, or
        // the machine hasn't been relogged since), and answering only
        // "is the file right" would leave the job dormant until next
        // login.
        let current = std::fs::read_to_string(&path).unwrap_or_default();
        if current != body {
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

        // Register right away so the user doesn't need a relogin to
        // get coverage from this session on. `bootstrap` on an
        // already-registered job errors — harmless.
        if let Some(uid) = uid() {
            let target = format!("gui/{uid}");
            launchctl(&["bootout", &format!("{target}/{APP_ID}")]);
            launchctl(&["bootstrap", &target, &path.to_string_lossy()]);
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn sync(_enabled: bool) {
        // Not implemented on this platform yet — the checkbox keeps
        // editing config.toml only, same contract as before.
    }
}

/// Make the OS autostart entry match the setting. Cheap and
/// idempotent — call at startup and after every settings reload.
pub(crate) fn sync(enabled: bool) {
    imp::sync(enabled);
}
