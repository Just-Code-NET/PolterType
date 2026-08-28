//! Best-effort "does the OS prefer a dark UI?" detection.
//!
//! Every probe shells out to a canonical CLI tool rather than adding a
//! D-Bus, Objective-C or registry dependency. A tool that does not
//! exist on this platform answers `None`, which is what keeps the
//! module free of `#[cfg(target_os)]` — and why the order below is
//! only a preference, not a policy.
//!
//! iced does not answer this for us. Its `auto-detect-theme` goes
//! through `dark-light` 1.x, which is broken twice on Linux: the
//! XDG-portal probe deserialises the D-Bus reply as a bare `u32` while
//! the portal wraps the value in a nested variant, and the fallback
//! knows only GNOME/KDE-family desktops.

use std::process::Command;

/// True when the OS prefers a dark UI. Sampled once at window start.
///
/// iced 0.13's `Theme::default()` used to answer for Windows and
/// macOS, so this only ever probed Linux; in 0.14 the same name is a
/// trait method that maps a preference iced was *given* onto a theme
/// and detects nothing on its own. Nobody noticed for two releases
/// because on Linux the portal had always been the real authority —
/// while `ui_theme = "system"` had quietly meant "light" everywhere
/// else (issue #43).
pub fn system_prefers_dark() -> bool {
    portal_prefers_dark()
        .or_else(gsettings_prefers_dark)
        .or_else(macos_prefers_dark)
        .or_else(windows_prefers_dark)
        .unwrap_or(false)
}

/// macOS writes the global domain key `AppleInterfaceStyle` only while
/// dark is on, and `defaults` exits non-zero when it is absent — so
/// "light" and "the key is missing" are the same answer, and both are
/// `Some(false)` here rather than `None`: on a Mac this probe is
/// authoritative either way.
fn macos_prefers_dark() -> Option<bool> {
    let out = Command::new("defaults")
        .args(["read", "-g", "AppleInterfaceStyle"])
        .output()
        .ok()?;
    if !out.status.success() {
        return Some(false);
    }
    Some(
        String::from_utf8_lossy(&out.stdout)
            .trim()
            .eq_ignore_ascii_case("dark"),
    )
}

/// Windows keeps the app-level preference in the Personalize key as
/// `AppsUseLightTheme` — 0 is dark, 1 is light. `SystemUsesLightTheme`
/// next to it is the taskbar's, not ours.
fn windows_prefers_dark() -> Option<bool> {
    let out = Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
            "/v",
            "AppsUseLightTheme",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_reg_apps_use_light(&String::from_utf8_lossy(&out.stdout))
}

/// Parse `reg query … /v AppsUseLightTheme` output, whose value line
/// ends in `REG_DWORD    0x0`.
pub(super) fn parse_reg_apps_use_light(reply: &str) -> Option<bool> {
    let raw = reply
        .lines()
        .find(|l| l.contains("AppsUseLightTheme"))?
        .split_whitespace()
        .last()?;
    let digits = raw.strip_prefix("0x").unwrap_or(raw);
    u32::from_str_radix(digits, 16).ok().map(|v| v == 0)
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
