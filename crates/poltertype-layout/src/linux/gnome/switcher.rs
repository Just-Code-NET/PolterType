//! `GnomeSwitcher` — layout control via gsettings.

use super::*;
use crate::linux::shared::xkb_to_bcp47;
use crate::{LayoutError, LayoutId, LayoutSwitcher};
use std::process::Command;
use tracing::{debug, warn};

pub struct GnomeSwitcher;

pub fn try_init() -> Option<GnomeSwitcher> {
    init(UnreadSchema::StandDown)
}

/// `try_init` without the "this desktop ignores the schema" check —
/// for `POLTERTYPE_LAYOUT_BACKEND=gnome`. The check is a list of
/// desktop names, so it can only ever be as right as the reports
/// behind it; a user whose gsettings switching demonstrably works
/// needs a way to say so that our list cannot override.
pub fn init_without_desktop_check() -> Option<GnomeSwitcher> {
    init(UnreadSchema::Ignore)
}

fn init(unread: UnreadSchema) -> Option<GnomeSwitcher> {
    let exists = Command::new("gsettings")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !exists {
        return None;
    }
    // Reading the value, not the exit status: the schema ships with
    // GTK, so machines running no GNOME-family desktop have it too, and
    // there `sources` reads back empty. Claiming the session on that
    // basis shadows the X11/XKB backend that would have worked.
    let sources = read_sources().unwrap_or_default();
    if sources.is_empty() {
        return None;
    }
    // Populated is still not the same as read: Cinnamon populates this
    // schema and drives the layout elsewhere (#26). It is probed before
    // this backend and has already had its chance by now — this is the
    // guard for the paths that reach us anyway.
    if matches!(unread, UnreadSchema::StandDown) && crate::linux::cinnamon::session_is_cinnamon() {
        debug!(
            "input-sources schema is populated but Cinnamon does not read it; \
             standing down rather than writing a key nobody acts on"
        );
        return None;
    }
    // The wlroots compositors keep their xkb configuration themselves
    // and read no schema at all. Measured on labwc, 2026-08-24: the
    // write returned success, the keyboard never changed group, and the
    // engine then read our own write back and decided the layout was
    // already the one it wanted — the #26 failure exactly. Standing
    // down leaves "layout switching is off", which is true and visible,
    // instead of a switch that silently does nothing.
    if matches!(unread, UnreadSchema::StandDown) && session_is_wlroots() {
        debug!(
            "input-sources schema is populated but this wlroots compositor does not read it; \
             standing down rather than writing a key nobody acts on"
        );
        return None;
    }
    Some(GnomeSwitcher)
}

impl LayoutSwitcher for GnomeSwitcher {
    fn current(&self) -> Result<LayoutId, LayoutError> {
        let sources = read_sources()?;
        let idx = read_current_index()?;
        sources
            .get(idx as usize)
            .cloned()
            .ok_or_else(|| LayoutError::Os(format!("current index {idx} out of range")))
    }

    fn list_active(&self) -> Result<Vec<LayoutId>, LayoutError> {
        read_sources()
    }

    fn switch_to(&self, id: &LayoutId) -> Result<(), LayoutError> {
        let sources = read_sources()?;
        let Some(idx) = sources.iter().position(|s| s == id) else {
            return Err(LayoutError::NotActive(id.clone()));
        };
        let status = Command::new("gsettings")
            .args(["set", SCHEMA, "current", &idx.to_string()])
            .status()
            .map_err(|e| LayoutError::Os(format!("gsettings spawn: {e}")))?;
        if !status.success() {
            return Err(LayoutError::Os(format!("gsettings set returned {status}")));
        }
        debug!(layout = %id, idx, "GNOME layout switched");
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        // Named for what we shell out to: one tag for every desktop this
        // backend can end up driving.
        "linux-gsettings"
    }
}

/// Compositors that own their xkb configuration outright and read no
/// settings schema — so a populated `input-sources` there was populated
/// by something else, and writing to it moves nothing.
///
/// Hyprland is deliberately absent: it is one of these, but it has its
/// own backend and is probed long before this one.
const WLROOTS_NAMES: [&str; 6] = ["wlroots", "labwc", "sway", "river", "niri", "wayfire"];

fn session_is_wlroots() -> bool {
    crate::linux::cinnamon::DESKTOP_VARS.iter().any(|var| {
        std::env::var(var).is_ok_and(|value| {
            value.split(':').any(|entry| {
                let entry = entry.trim().rsplit('/').next().unwrap_or_default();
                WLROOTS_NAMES
                    .iter()
                    .any(|known| entry.eq_ignore_ascii_case(known))
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `XDG_CURRENT_DESKTOP` on the guest reads `labwc:wlroots`, and the
    /// entry is matched whole — `sway-something` is a fork whose input
    /// stack nobody here has seen.
    #[test]
    fn wlroots_sessions_are_recognised_by_either_half_of_the_name() {
        for value in ["labwc:wlroots", "wlroots", "sway", "SWAY", "river", "niri"] {
            // SAFETY: single-threaded test, and the variable is restored
            // by the next iteration or the removal below.
            unsafe { std::env::set_var("XDG_CURRENT_DESKTOP", value) };
            assert!(session_is_wlroots(), "{value} should read as wlroots");
        }
        for value in ["GNOME", "ubuntu:GNOME", "KDE", "swaything"] {
            unsafe { std::env::set_var("XDG_CURRENT_DESKTOP", value) };
            assert!(!session_is_wlroots(), "{value} should not read as wlroots");
        }
        unsafe { std::env::remove_var("XDG_CURRENT_DESKTOP") };
    }
}
