//! Two holes closed here: `iced` passed winit an empty
//! `application_id` (an empty X11 `WM_CLASS` is worse than none —
//! winit's `argv[0]` fallback runs only when nothing is passed at all),
//! and on Wayland a `.desktop` entry is the *only* route an icon has,
//! as winit's `set_window_icon` there is an empty function. Packaged
//! installs already ship the entry, so what is left to cover is an
//! un-integrated AppImage and `cargo run`. See docs/DECISIONS.md,
//! 2026-08-16.

use std::path::{Path, PathBuf};

use tracing::{debug, info, warn};

use crate::consts::{DESKTOP_ID, HICOLOR_SIZES};

/// The `platform_specific` field of an `iced` window.
///
/// Here and not in the binary because the type behind
/// `iced::window::settings::PlatformSpecific` is a *different struct*
/// on each platform — `application_id` exists only in the Linux one —
/// so merely naming the field is `#[cfg]` code, and `poltertype-app`
/// holds none.
pub fn window_platform_specific() -> iced::window::settings::PlatformSpecific {
    iced::window::settings::PlatformSpecific {
        application_id: DESKTOP_ID.to_owned(),
        ..Default::default()
    }
}

/// Directories a distribution package would have put the entry in.
const DEFAULT_DATA_DIRS: &str = "/usr/local/share:/usr/share";

/// Make sure this app has a `.desktop` entry and an icon the desktop
/// can find, writing them into the user's own data directory if
/// nothing else has.
///
/// Best-effort from top to bottom: every failure here costs an icon,
/// and refusing to start because we could not draw ourselves in a menu
/// would be the worse bug. Deliberately not gated on a setting, unlike
/// autostart — see docs/DECISIONS.md, 2026-08-16.
pub fn install_desktop_entry() {
    if let Some(path) = packaged_entry() {
        debug!(?path, "a packaged desktop entry owns this app; leaving it");
        return;
    }
    let Some(data_home) = data_home() else {
        warn!("could not resolve XDG_DATA_HOME; the app will have no desktop icon");
        return;
    };
    let Some(exec) = exec_target() else {
        warn!("could not resolve this executable's path; skipping the desktop entry");
        return;
    };
    install_into(&data_home, &exec);
}

/// The half of [`install_desktop_entry`] that touches files, with what
/// it resolves from the environment passed in instead — split off to
/// be testable, since the alternative sets `XDG_DATA_HOME` and
/// `std::env::set_var` is `unsafe` in this edition, in a crate that
/// forbids `unsafe`.
///
/// Returns whether anything was written.
pub(crate) fn install_into(data_home: &Path, exec: &Path) -> bool {
    let body = entry_body(exec);
    let entry = data_home
        .join("applications")
        .join(format!("{DESKTOP_ID}.desktop"));
    // The version stamped into `body` makes an ordinary launch one
    // read-and-compare and an upgrade a full rewrite — which is how a
    // mark redrawn in a later version reaches existing installs.
    if std::fs::read_to_string(&entry).is_ok_and(|current| current == body) {
        return false;
    }

    // Icons first: dying between the two leaves the entry stale, so
    // the next launch retries. The reverse order would stamp "done"
    // over a half-installed icon theme.
    write_icons(data_home);

    if let Err(e) = write_file(&entry, body.as_bytes()) {
        warn!(?e, path = ?entry, "could not write the desktop entry");
        return false;
    }
    info!(path = ?entry, "installed the desktop entry");
    true
}

/// The entry a distribution package would have installed, if any: ours
/// in `XDG_DATA_HOME` would take precedence over a file the package
/// manager keeps up to date.
///
/// Deliberately blind to the AppImage integrators (AppImageLauncher,
/// Gear Lever), whose entries use a mangled stem
/// (`appimagekit_<hash>-poltertype.desktop`). Guessing at that naming
/// and skipping wrongly is the original bug back; not matching them
/// costs one duplicate menu entry that launches the same file.
fn packaged_entry() -> Option<PathBuf> {
    let dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_DATA_DIRS.to_owned());
    dirs.split(':')
        .filter(|d| !d.is_empty())
        .map(|d| {
            Path::new(d)
                .join("applications")
                .join(format!("{DESKTOP_ID}.desktop"))
        })
        .find(|p| p.exists())
}

/// `$XDG_DATA_HOME`, or the `~/.local/share` the spec falls back to.
/// A relative `XDG_DATA_HOME` counts as unset, per the spec.
fn data_home() -> Option<PathBuf> {
    match std::env::var_os("XDG_DATA_HOME") {
        Some(v) if Path::new(&v).is_absolute() => Some(PathBuf::from(v)),
        _ => Some(PathBuf::from(std::env::var_os("HOME")?).join(".local/share")),
    }
}

/// What the entry's `Exec` should launch.
///
/// `$APPIMAGE` before `current_exe`: inside a running AppImage the
/// latter points into the mount (`/tmp/.mount_XXXXXX/usr/bin/...`),
/// which is gone the moment the app exits.
fn exec_target() -> Option<PathBuf> {
    match std::env::var_os("APPIMAGE") {
        Some(v) if Path::new(&v).is_absolute() => Some(PathBuf::from(v)),
        _ => std::env::current_exe().ok(),
    }
}

/// Quote a program path for a Desktop Entry `Exec=` value.
///
/// Twin of `poltertype_autostart::linux::exec_quote`, kept apart so
/// neither crate depends on the other for five lines of string
/// handling. Change one, change both.
pub(crate) fn exec_quote(exe: &Path) -> String {
    let escaped = exe
        .display()
        .to_string()
        .replace('\\', r"\\")
        .replace('"', r#"\""#)
        .replace('`', r"\`")
        .replace('$', r"\$");
    format!("\"{escaped}\"")
}

/// The full text of the entry.
///
/// Every key except `Exec` matches `installers/linux/poltertype.desktop`
/// byte for byte, and a test in this crate holds them to it.
/// `X-PolterType-Version` is what makes an upgrade rewrite the icons.
pub(crate) fn entry_body(exec: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=PolterType\n\
         GenericName=Keyboard Layout Switcher\n\
         Comment=Auto-detect and fix wrong-keyboard-layout typing\n\
         Exec={exec}\n\
         Icon={DESKTOP_ID}\n\
         Terminal=false\n\
         Categories=Utility;System;\n\
         Keywords=keyboard;layout;language;input;\n\
         StartupNotify=false\n\
         StartupWMClass={DESKTOP_ID}\n\
         NoDisplay=false\n\
         X-PolterType-Version={version}\n",
        exec = exec_quote(exec),
        version = env!("CARGO_PKG_VERSION"),
    )
}

/// Render the mark into the user's `hicolor` theme. A failed size is
/// skipped rather than aborting the rest — the remaining sizes still
/// draw everywhere, by scaling.
fn write_icons(data_home: &Path) {
    for &size in HICOLOR_SIZES {
        let path = data_home
            .join("icons/hicolor")
            .join(format!("{size}x{size}"))
            .join("apps")
            .join(format!("{DESKTOP_ID}.png"));
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                warn!(?e, ?parent, "could not create the icon directory");
                continue;
            }
        }
        if let Err(e) = poltertype_icon::render_png(size, &path) {
            warn!(?e, ?path, size, "could not write the app icon");
        }
    }
}

/// Write the file by creating a sibling and renaming it into place —
/// rewriting in place doesn't move the `applications` directory's
/// mtime, which is what some desktops key their menu cache on. See
/// docs/DECISIONS.md, 2026-08-30.
fn write_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;

    // Hidden and pid-stamped: two instances starting at once must not
    // write the same temporary file, and a desktop reading the
    // directory mid-write must not take it for an entry of its own.
    let temp = parent.join(format!(".{DESKTOP_ID}.{}.tmp", std::process::id()));
    std::fs::write(&temp, bytes)?;
    if let Err(e) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(e);
    }
    Ok(())
}
