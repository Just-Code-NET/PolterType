//! Linux: an XDG autostart entry.
//!
//! `$XDG_CONFIG_HOME/autostart/<id>.desktop` (falling back to
//! `~/.config`) is the one autostart mechanism every desktop
//! environment we care about honours — GNOME, KDE, XFCE, Cinnamon,
//! and the wlroots compositors via their own session handling. No
//! DE-specific branch is needed, which is a pleasant change from
//! layout switching.
//!
//! There is nothing to register: the file *is* the registration, read
//! at session start. So unlike macOS this backend has no "load it now
//! so the user need not log out" step, and no way to kill the running
//! app either.

use std::path::{Path, PathBuf};

use tracing::{debug, warn};

use crate::types::App;

fn autostart_dir() -> Option<PathBuf> {
    // XDG_CONFIG_HOME wins when set to an absolute path; the spec says
    // to ignore it otherwise.
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(v) if Path::new(&v).is_absolute() => PathBuf::from(v),
        _ => PathBuf::from(std::env::var_os("HOME")?).join(".config"),
    };
    Some(base.join("autostart"))
}

/// Quote a program path for a Desktop Entry `Exec=` value.
///
/// The spec reserves a pile of characters in `Exec` and requires
/// backslash-escaping inside double quotes. Paths with spaces are the
/// common case (`/home/user/My Apps/…`); the rest matter because a
/// home directory is user-named and can contain almost anything.
pub(crate) fn exec_quote(exe: &Path) -> String {
    let raw = exe.display().to_string();
    let escaped = raw
        .replace('\\', r"\\")
        .replace('"', r#"\""#)
        .replace('`', r"\`")
        .replace('$', r"\$");
    format!("\"{escaped}\"")
}

pub(crate) fn desktop_body(app: App<'_>, exe: &Path) -> String {
    // `Name` lands in the DE's own "Startup Applications" list, so it
    // is the human name, not the id. Terminal=false keeps a terminal
    // emulator from being spawned around a tray app.
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={name}\n\
         Comment=Fix text typed in the wrong keyboard layout\n\
         Exec={exec}\n\
         Terminal=false\n\
         NoDisplay=true\n\
         X-GNOME-Autostart-enabled=true\n",
        name = app.name,
        exec = exec_quote(exe),
    )
}

pub(crate) fn sync(enabled: bool, app: App<'_>) {
    let Some(dir) = autostart_dir() else {
        warn!("could not resolve the XDG autostart directory; autostart unchanged");
        return;
    };
    let path = dir.join(format!("{}.desktop", app.id));

    if !enabled {
        if path.exists() {
            match std::fs::remove_file(&path) {
                Ok(()) => debug!(?path, "autostart disabled: entry removed"),
                Err(e) => warn!(?e, ?path, "could not remove autostart entry"),
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
    let body = desktop_body(app, &exe);

    // Rewrite only on drift, so a config reload does not churn the
    // file (and the DE's file watchers) on every settings save.
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
