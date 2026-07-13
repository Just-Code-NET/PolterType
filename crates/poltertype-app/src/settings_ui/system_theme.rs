//! Best-effort "does the OS prefer a dark UI?" detection.
//!
//! iced's `auto-detect-theme` feature answers this through the
//! `dark-light` crate (v1.x), which is broken in two ways on Linux:
//! its XDG-portal probe deserialises the D-Bus reply as a bare `u32`
//! while the portal wraps the value in a nested variant (so the probe
//! *always* fails even when the portal answers correctly), and its
//! fallback only knows GNOME/KDE-family desktops — on anything else
//! (Hyprland, sway, …) it reports "light". Windows and macOS paths
//! work fine.
//!
//! So: trust `Theme::default()` when it says dark, and when it says
//! light — which is also what a failed detection looks like — double-
//! check the freedesktop portal ourselves, then the GNOME gsettings
//! key. Both probes shell out to canonical CLI tools (`busctl`,
//! `gsettings`) per the workspace convention of avoiding a zbus
//! dependency; on Windows/macOS the binaries simply don't exist and
//! both probes return `None`, keeping this module free of
//! `#[cfg(target_os)]`.

use std::process::Command;

use iced::Theme;

/// True when the OS prefers a dark UI. Sampled once at window start.
pub fn system_prefers_dark() -> bool {
    if matches!(Theme::default(), Theme::Dark) {
        return true;
    }
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
