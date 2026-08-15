//! Telling a Linux desktop which application a window belongs to.
//!
//! Windows and macOS answer "what is this app called and what does it
//! look like?" out of the executable itself — a resource block inside
//! `poltertype.exe`, a `Resources/AppIcon.icns` inside the `.app`.
//! Linux answers it out of a *third* file: a `.desktop` entry in a
//! shared directory, found by the id the window declares. v0.17.1 gave
//! the first two what they needed and left this one as it was, which
//! turns out to be two separate holes.
//!
//! **A window has to say which entry is its own.** `iced` passes its
//! `application_id` straight through to winit, and ours was never set,
//! so what got passed was the empty string — an empty Wayland `app_id`
//! and, worse, an empty X11 `WM_CLASS`: winit's fallback to `argv[0]`
//! only runs when nothing is passed *at all*, and `Some("")` is not
//! nothing. Measured on Hyprland before the fix, `hyprctl clients`
//! reported `class: ""` for the Settings window.
//!
//! **On Wayland that entry is the only place an icon can come from.**
//! winit's Wayland backend implements `set_window_icon` as an empty
//! function — the icon [`crate::window_platform_specific`]'s caller
//! builds is dropped on the floor — because the protocol has no
//! window-icon concept to implement. X11 takes `_NET_WM_ICON` and is
//! fine either way, so this is a Wayland hole with an X11 half: there,
//! the icon showed but the empty `WM_CLASS` still broke matching the
//! window to an entry, which is what pinning and grouping run on.
//!
//! Packaged installs already ship the entry — the AUR package installs
//! `poltertype.desktop` and the mark into `hicolor`, and so would a
//! `.deb` — so what is left to cover is the un-packaged case: a
//! downloaded AppImage that nobody integrated, and a developer's
//! `cargo run`. Both are ordinary ways to run this app, and neither
//! puts a single file anywhere the desktop looks. Hence
//! [`install_desktop_entry`], and hence its first act being to check
//! whether a package got there first.

#[cfg(target_os = "linux")]
use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
use tracing::{debug, info, warn};

/// The id this app's windows carry, and the stem of the `.desktop`
/// entry they are matched against.
///
/// Deliberately **not** `APP_ID` (`dev.opensource.poltertype`), which
/// names the autostart entry and the instance lock. What this string
/// has to agree with is the entry the *packages* install, and both the
/// AppImage and the AUR `PKGBUILD` have shipped `poltertype.desktop`
/// since before this module existed. Switching those to the
/// reverse-DNS form to match the other id would rename a file every
/// installed copy already has, to fix nothing.
///
/// It is also the `Icon=` value and therefore the icon's name inside
/// the `hicolor` theme: one string, so a window, a menu entry and a
/// notification cannot disagree about which app they are showing.
pub const DESKTOP_ID: &str = "poltertype";

/// The `platform_specific` field of an `iced` window, filled in for
/// the platform this build targets.
///
/// Here and not in the binary because the type behind
/// `iced::window::settings::PlatformSpecific` is a *different struct*
/// on each platform — `application_id` exists only in the Linux one —
/// so merely naming the field is `#[cfg]` code, and `poltertype-app`
/// holds none.
#[cfg(target_os = "linux")]
pub fn window_platform_specific() -> iced::window::settings::PlatformSpecific {
    iced::window::settings::PlatformSpecific {
        application_id: DESKTOP_ID.to_owned(),
        ..Default::default()
    }
}

/// Windows and macOS identify a window's application by the binary it
/// came from, so there is nothing to declare.
#[cfg(not(target_os = "linux"))]
pub fn window_platform_specific() -> iced::window::settings::PlatformSpecific {
    iced::window::settings::PlatformSpecific::default()
}

/// Icon sizes written into the user's `hicolor` theme.
///
/// Each one is rendered from the geometry rather than filtered down
/// from a single master — the same reason the Windows icon resource
/// carries six sizes instead of one. 16 is absent because
/// `poltertype-icon` refuses to write a PNG below `MIN_PNG_SIZE`, and
/// nothing in a menu, a switcher or a notification asks for one.
#[cfg(target_os = "linux")]
pub(crate) const HICOLOR_SIZES: &[u32] = &[32, 48, 64, 128, 256];

/// Directories a distribution package would have put the entry in.
#[cfg(target_os = "linux")]
const DEFAULT_DATA_DIRS: &str = "/usr/local/share:/usr/share";

/// Make sure this app has a `.desktop` entry and an icon the desktop
/// can find, writing them into the user's own data directory if
/// nothing else has.
///
/// Best-effort from top to bottom: every failure here costs an icon,
/// and an app that refuses to start because it could not draw itself
/// in a menu would be a worse bug than the one this fixes.
///
/// Not gated on a setting, unlike autostart. Autostart changes what
/// the machine does; this only answers a question the desktop is
/// already asking, and the answer to "may PolterType tell your
/// desktop what PolterType looks like?" is not interesting enough to
/// put in a config file.
#[cfg(target_os = "linux")]
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

/// The half of [`install_desktop_entry`] that touches files, with the
/// two things it resolves from the environment passed in instead.
///
/// Split off to be testable: the alternative is a test that sets
/// `XDG_DATA_HOME`, and `std::env::set_var` is `unsafe` in this
/// edition — in a crate that forbids `unsafe` outright.
///
/// Returns whether anything was written, which is also what the test
/// checks the second call for.
#[cfg(target_os = "linux")]
pub(crate) fn install_into(data_home: &Path, exec: &Path) -> bool {
    let body = entry_body(exec);
    let entry = data_home
        .join("applications")
        .join(format!("{DESKTOP_ID}.desktop"));
    // The stamped version inside `body` is what makes this cheap: an
    // ordinary launch reads one small file, compares it and stops.
    // A new release, or an AppImage the user moved, changes the text
    // and everything is rewritten — including the icons, which is why
    // a mark redrawn in some later version reaches existing installs.
    if std::fs::read_to_string(&entry).is_ok_and(|current| current == body) {
        return false;
    }

    // Icons first. If we die between the two, the entry is still
    // absent or stale, so the next launch tries again — the reverse
    // order would stamp "done" over a half-installed icon theme.
    write_icons(data_home);

    if let Err(e) = write_file(&entry, body.as_bytes()) {
        warn!(?e, path = ?entry, "could not write the desktop entry");
        return false;
    }
    info!(path = ?entry, "installed the desktop entry");
    true
}

/// Nothing to install: the executable already carries its own identity.
#[cfg(not(target_os = "linux"))]
pub fn install_desktop_entry() {}

/// The entry a distribution package would have installed, if there is
/// one.
///
/// Its presence means the entry, the icon and the `Exec` line are
/// somebody else's job — and a copy of ours in `XDG_DATA_HOME` would
/// take precedence over theirs, which is the wrong way round for a
/// file the package manager keeps up to date.
#[cfg(target_os = "linux")]
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
///
/// Same shape as `poltertype-autostart`'s config-side twin, including
/// ignoring a relative `XDG_DATA_HOME` — the spec says to treat one as
/// unset rather than resolve it against the working directory.
#[cfg(target_os = "linux")]
fn data_home() -> Option<PathBuf> {
    match std::env::var_os("XDG_DATA_HOME") {
        Some(v) if Path::new(&v).is_absolute() => Some(PathBuf::from(v)),
        _ => Some(PathBuf::from(std::env::var_os("HOME")?).join(".local/share")),
    }
}

/// What the entry's `Exec` should launch.
///
/// `$APPIMAGE` before `current_exe`, because inside a running AppImage
/// the executable's path points into the mount
/// (`/tmp/.mount_XXXXXX/usr/bin/poltertype`) and that path is gone the
/// moment the app exits. The AppImage runtime exports the path of the
/// file the user actually downloaded, which is the only one that still
/// exists tomorrow.
#[cfg(target_os = "linux")]
fn exec_target() -> Option<PathBuf> {
    match std::env::var_os("APPIMAGE") {
        Some(v) if Path::new(&v).is_absolute() => Some(PathBuf::from(v)),
        _ => std::env::current_exe().ok(),
    }
}

/// Quote a program path for a Desktop Entry `Exec=` value.
///
/// Twin of `poltertype_autostart::linux::exec_quote` — same spec, same
/// escapes, and the two are expected to agree; they are apart because
/// neither crate should depend on the other for five lines of string
/// handling. Change one, change both.
#[cfg(target_os = "linux")]
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
/// byte for byte, and a test in this crate reads that file and holds
/// them to it — a menu entry that disagreed with the packaged one about
/// the app's own name is exactly the drift nobody would notice.
///
/// `X-PolterType-Version` is not decoration: it is what makes an
/// upgrade rewrite the icons. See [`install_desktop_entry`].
#[cfg(target_os = "linux")]
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

/// Render the mark into the user's `hicolor` theme.
///
/// One failed size is reported and skipped rather than aborting the
/// rest: a theme with four of five sizes still draws an icon
/// everywhere, by scaling.
#[cfg(target_os = "linux")]
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

/// Write `bytes` to `path`, creating the directory above it.
#[cfg(target_os = "linux")]
fn write_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)
}
