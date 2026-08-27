//! Best-effort "does the OS prefer a dark UI?" detection.
//!
//! iced's `auto-detect-theme` answers through `dark-light` 1.x, which
//! is broken twice on Linux: its XDG-portal probe deserialises the
//! D-Bus reply as a bare `u32` while the portal wraps the value in a
//! nested variant, so the probe always fails even when the portal
//! answers; and its fallback knows only GNOME/KDE-family desktops, so
//! anything else reports "light". Windows and macOS are fine.
//!
//! So: trust `Theme::default()` when it says dark, and when it says
//! light — which is also what a failed detection looks like — check the
//! freedesktop portal ourselves, then the GNOME gsettings key. Both
//! probes shell out to canonical CLI tools rather than adding a zbus
//! dependency; on Windows and macOS those binaries do not exist and
//! both return `None`, which keeps this module free of
//! `#[cfg(target_os)]`.

use std::process::Command;

/// True when the OS prefers a dark UI. Sampled once at window start.
///
/// iced 0.13's `Theme::default()` used to answer this first; in 0.14
/// the same name is a trait method that maps a preference iced was
/// *given* onto a theme, and detects nothing on its own. The portal is
/// the authority here anyway, and always was — the old probe only ever
/// short-circuited when it agreed.
pub fn system_prefers_dark() -> bool {
    portal_prefers_dark()
        .or_else(gsettings_prefers_dark)
        .unwrap_or(false)
}

/// Ask the XDG desktop portal for `org.freedesktop.appearance`
/// `color-scheme` (0 = no preference, 1 = prefer dark, 2 = prefer
/// light) via `busctl`. Returns `None` when the tool or the portal
/// is unavailable, or the answer is "no preference".
fn portal_prefers_dark() -> Option<bool> {
    let out = Command::new("busctl")
        .args([
            "--user",
            "--timeout=1",
            "call",
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.Settings",
            "Read",
            "ss",
            "org.freedesktop.appearance",
            "color-scheme",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_portal_reply(&String::from_utf8_lossy(&out.stdout))
}

/// Parse `busctl call … Read` output — a typed rendering like
/// `v v u 1` where the trailing number is the color-scheme value.
pub(super) fn parse_portal_reply(reply: &str) -> Option<bool> {
    match reply.split_whitespace().last()? {
        "1" => Some(true),
        "2" => Some(false),
        _ => None,
    }
}

/// Fall back to the GNOME-stack setting many distros carry even
/// outside GNOME (`gsettings` ships with GLib): `prefer-dark` etc.
fn gsettings_prefers_dark() -> Option<bool> {
    let out = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_color_scheme(&String::from_utf8_lossy(&out.stdout))
}

/// Parse `gsettings get … color-scheme` output (`'prefer-dark'`).
/// Only an explicit preference produces an answer — `'default'`
/// means "no preference", not "light".
pub(super) fn parse_color_scheme(value: &str) -> Option<bool> {
    let v = value.trim().to_ascii_lowercase();
    if v.contains("prefer-dark") {
        Some(true)
    } else if v.contains("prefer-light") {
        Some(false)
    } else {
        None
    }
}
