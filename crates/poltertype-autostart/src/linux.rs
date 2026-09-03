//! Linux: a systemd user service, or an XDG autostart entry as the
//! fallback for a machine with no user manager. See docs/DECISIONS.md,
//! 2026-08-21, for why a `.service` under `graphical-session.target`
//! rather than the XDG mechanism alone.
//!
//! There is nothing to register in the XDG fallback — the file *is*
//! the registration, read at session start.

use std::path::{Path, PathBuf};
use std::process::Command;

use tracing::{debug, info, warn};

use crate::types::App;

fn config_home() -> Option<PathBuf> {
    // XDG_CONFIG_HOME wins when set to an absolute path; the spec says
    // to ignore it otherwise.
    match std::env::var_os("XDG_CONFIG_HOME") {
        Some(v) if Path::new(&v).is_absolute() => Some(PathBuf::from(v)),
        _ => Some(PathBuf::from(std::env::var_os("HOME")?).join(".config")),
    }
}

fn autostart_dir() -> Option<PathBuf> {
    Some(config_home()?.join("autostart"))
}

fn unit_dir() -> Option<PathBuf> {
    Some(config_home()?.join("systemd/user"))
}

/// The session target a desktop session brings up once it has an
/// environment worth inheriting.
const SESSION_TARGET: &str = "graphical-session.target";

/// Quote a program path for a Desktop Entry `Exec=` value.
///
/// The spec reserves a pile of characters and requires
/// backslash-escaping inside double quotes; a home directory is
/// user-named and can contain almost any of them. Twin of
/// `poltertype_shell::desktop::exec_quote` — change one, change both.
pub(crate) fn exec_quote(exe: &Path) -> String {
    let raw = exe.display().to_string();
    let escaped = raw
        .replace('\\', r"\\")
        .replace('"', r#"\""#)
        .replace('`', r"\`")
        .replace('$', r"\$");
    format!("\"{escaped}\"")
}

/// Quote a program path for a systemd `ExecStart=`.
///
/// Not the same rules as a desktop entry, which is why this is its own
/// function: systemd takes `\` and `"` inside double quotes the same
/// way, has no use for a backtick, and reads `$` as the start of a
/// variable reference — where the escape is a doubled `$`, not a
/// backslash.
pub(crate) fn systemd_quote(exe: &Path) -> String {
    let escaped = exe
        .display()
        .to_string()
        .replace('\\', r"\\")
        .replace('"', r#"\""#)
        .replace('$', "$$");
    format!("\"{escaped}\"")
}

/// What the entry's `Exec` should launch.
///
/// `$APPIMAGE` before `current_exe`: inside a running AppImage the
/// latter points into the mount (`/tmp/.mount_XXXXXX/usr/bin/...`),
/// which is gone by the time the session that reads this entry starts.
/// Twin of `poltertype_shell::desktop::exec_target`.
fn exec_target() -> Option<PathBuf> {
    match std::env::var_os("APPIMAGE") {
        Some(v) if Path::new(&v).is_absolute() => Some(PathBuf::from(v)),
        _ => std::env::current_exe().ok(),
    }
}

pub(crate) fn desktop_body(app: App<'_>, exe: &Path) -> String {
    // `Name` and `Icon` land in the DE's own "Startup Applications"
    // list — the only reason a NoDisplay entry carries them at all.
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={name}\n\
         Comment=Fix text typed in the wrong keyboard layout\n\
         Exec={exec}\n\
         Icon={icon}\n\
         Terminal=false\n\
         NoDisplay=true\n\
         X-GNOME-Autostart-enabled=true\n",
        name = app.name,
        icon = app.icon,
        exec = exec_quote(exe),
    )
}

pub(crate) fn unit_body(app: App<'_>, exe: &Path) -> String {
    // `PartOf` as well as `WantedBy`: leaving the session should stop
    // the app, or a second login starts a second one and the instance
    // lock turns that into a confusing "already running" in the log.
    //
    // No `Restart=`: the app exits deliberately (the tray's Quit, an
    // update installing itself), and systemd cannot tell that from a
    // crash without a policy that would fight both.
    format!(
        "[Unit]\n\
         Description={name}\n\
         Documentation=https://github.com/Just-Code-NET/PolterType\n\
         PartOf={target}\n\
         After={target}\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={exec}\n\
         \n\
         [Install]\n\
         WantedBy={target}\n",
        name = app.name,
        target = SESSION_TARGET,
        exec = systemd_quote(exe),
    )
}

fn unit_name(app: App<'_>) -> String {
    format!("{}.service", app.id)
}

/// Run `systemctl --user …`, `true` on a clean exit.
fn systemctl(args: &[&str]) -> bool {
    match Command::new("systemctl").arg("--user").args(args).output() {
        Ok(out) if out.status.success() => true,
        Ok(out) => {
            debug!(
                ?args,
                status = ?out.status,
                stderr = %String::from_utf8_lossy(&out.stderr).trim(),
                "systemctl --user"
            );
            false
        }
        Err(e) => {
            debug!(?e, ?args, "systemctl --user could not be run");
            false
        }
    }
}

/// Whether there is a user manager to install a unit into.
///
/// A property nobody reads, chosen because it answers "is systemd
/// there and talking to us" without side effects: a container, a
/// non-systemd distro or a session with no user instance all fail it.
fn systemd_available() -> bool {
    systemctl(&["show", "--property=Version"])
}

pub fn sync(enabled: bool, app: App<'_>) {
    if systemd_available() {
        sync_systemd(enabled, app);
    } else {
        debug!("no systemd user manager; falling back to the XDG autostart entry");
        sync_xdg(enabled, app);
    }
}

fn sync_systemd(enabled: bool, app: App<'_>) {
    let Some(dir) = unit_dir() else {
        warn!("could not resolve the systemd user unit directory; autostart unchanged");
        return;
    };
    let unit = unit_name(app);
    let path = dir.join(&unit);

    if !enabled {
        if path.exists() {
            systemctl(&["disable", &unit]);
            match std::fs::remove_file(&path) {
                Ok(()) => {
                    systemctl(&["daemon-reload"]);
                    debug!(?path, "autostart disabled: unit removed");
                }
                Err(e) => warn!(?e, ?path, "could not remove the autostart unit"),
            }
        }
        // A machine that has been through an older release may still
        // carry the entry that release wrote.
        remove_xdg_entry(app);
        return;
    }

    let Some(exe) = exec_target() else {
        warn!("could not resolve own exe; autostart unchanged");
        return;
    };
    let body = unit_body(app, &exe);
    let drifted = std::fs::read_to_string(&path).unwrap_or_default() != body;
    if drifted {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            warn!(?e, ?dir, "could not create the systemd user unit directory");
            return;
        }
        if let Err(e) = std::fs::write(&path, &body) {
            warn!(?e, ?path, "could not write the autostart unit");
            return;
        }
        systemctl(&["daemon-reload"]);
    }

    // `enable` is idempotent but spawns a process, so it runs only when
    // the symlink it would create is missing.
    let wants = dir
        .join(format!("{SESSION_TARGET}.wants"))
        .join(unit.as_str());
    if drifted || !wants.exists() {
        if systemctl(&["enable", &unit]) {
            info!(?path, "autostart enabled: systemd user unit");
        } else {
            warn!(?path, "could not enable the autostart unit");
            return;
        }
    }

    // Two mechanisms would start two copies, and the second one loses
    // to the instance lock with a message that reads like a fault.
    remove_xdg_entry(app);

    // The unit is installed correctly and will still never run if
    // nothing in this session reaches the target it hangs off. Worth a
    // line in the log rather than a silent nothing at next login.
    if !systemctl(&["is-active", "--quiet", SESSION_TARGET]) {
        warn!(
            target = SESSION_TARGET,
            "this session does not start {SESSION_TARGET}, so the autostart unit will not run at \
             login — see docs/PERMISSIONS.md, 'Autostart on a bare compositor'"
        );
    }
}

fn remove_xdg_entry(app: App<'_>) {
    let Some(path) = autostart_dir().map(|d| d.join(format!("{}.desktop", app.id))) else {
        return;
    };
    if !path.exists() {
        return;
    }
    match std::fs::remove_file(&path) {
        Ok(()) => debug!(?path, "removed the XDG autostart entry"),
        Err(e) => warn!(?e, ?path, "could not remove the XDG autostart entry"),
    }
}

fn sync_xdg(enabled: bool, app: App<'_>) {
    let Some(dir) = autostart_dir() else {
        warn!("could not resolve the XDG autostart directory; autostart unchanged");
        return;
    };
    let path = dir.join(format!("{}.desktop", app.id));

    if !enabled {
        remove_xdg_entry(app);
        return;
    }

    let Some(exe) = exec_target() else {
        warn!("could not resolve own exe; autostart unchanged");
        return;
    };
    let body = desktop_body(app, &exe);

    // Rewrite only on drift, or every settings save churns the file and
    // the DE's watchers with it.
    if std::fs::read_to_string(&path).unwrap_or_default() == body {
        return;
    }
    if let Err(e) = std::fs::create_dir_all(&dir) {
        warn!(?e, ?dir, "could not create the XDG autostart directory");
        return;
    }
    match std::fs::write(&path, &body) {
        Ok(()) => debug!(?path, "autostart enabled: entry written"),
        Err(e) => warn!(?e, ?path, "could not write autostart entry"),
    }
}
