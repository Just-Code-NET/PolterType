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
    // Populated is still not the same as read, and the schema is a
    // *file* — once anything has written it, it stays written for every
    // later session on that machine. So the question is not "is this
    // key set" but "is anything here going to act on it".
    if matches!(unread, UnreadSchema::StandDown) && !session_reads_this_schema() {
        debug!(
            desktop = ?std::env::var("XDG_CURRENT_DESKTOP").ok(),
            "input-sources schema is populated but this desktop does not read it; \
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
    /// The shell's own most-recently-used head first, because `current`
    /// is only ever as true as the last thing that wrote it — and on
    /// GNOME 49 that is us, writing a key the shell ignores. Reading it
    /// there told the engine the user was typing in a layout they were
    /// not, which is wrong before any correction is even considered.
    fn current(&self) -> Result<LayoutId, LayoutError> {
        let sources = read_sources()?;
        if let Some(live) = read_live_source()
            && sources.contains(&live)
        {
            return Ok(live);
        }
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

    /// `mru-sources` is maintained by the shell, so it can disagree
    /// with the `current` we just wrote — and on GNOME 49 it does, every
    /// time. `None` when the key is unavailable, which keeps every
    /// older GNOME behaving exactly as before.
    fn verify_switched(&self, target: &LayoutId) -> Option<bool> {
        read_live_source().map(|live| live == *target)
    }

    /// GNOME 49 moves for nothing else. Both keys this backend can
    /// write are inert there, and the shell's own binding is the only
    /// mechanism that was measured to work.
    fn switch_chord(&self) -> Option<poltertype_types::SwitchChord> {
        read_switch_binding()
    }

    fn backend_name(&self) -> &'static str {
        // Named for what we shell out to: one tag for every desktop this
        // backend can end up driving.
        "linux-gsettings"
    }
}

/// Desktops whose own daemon applies `org.gnome.desktop.input-sources`.
///
/// A positive list, and that direction is the whole point. The negative
/// one — stand down for Cinnamon, for MATE, for wlroots — only ever
/// covered the desktops somebody had already been bitten by, and the
/// schema outlives the desktop that wrote it: `dconf` is a file in the
/// user's home, so a machine where GNOME once configured two layouts
/// keeps that key for every later session, including the ones that
/// never read it. Measured in the desktop matrix on 2026-08-27, on a
/// guest where an earlier GNOME run had populated it: fluxbox, i3,
/// icewm, LXQt, openbox and Xfce/X11 all took this backend, wrote the
/// key, watched their own session put the layout straight back, and
/// declined every correction — six sessions that had corrected fine on
/// the same code with the key empty. Clearing it put them back on
/// `linux-x11-xkb` and they worked.
///
/// So: name the desktops that *do* act on it, and let everything else
/// fall through to the backend that drives that session for real.
/// `POLTERTYPE_LAYOUT_BACKEND=gnome` overrides this for anyone whose
/// desktop we have not heard of and whose gsettings switching works.
///
/// Cinnamon (#26) and MATE, which used to have a branch each here, are
/// covered by this rule: neither names itself GNOME.
const GNOME_FAMILY_NAMES: [&str; 6] = [
    "gnome",
    "gnome-classic",
    "gnome-flashback",
    "unity",
    "budgie",
    "pantheon",
];

fn session_reads_this_schema() -> bool {
    session_is_named(&GNOME_FAMILY_NAMES)
}

/// Compositors that own their xkb configuration outright and read no
/// settings schema — so a populated `input-sources` there was populated
/// by something else, and writing to it moves nothing.
///
/// Hyprland is deliberately absent: it is one of these, but it has its
/// own backend and is probed long before this one.
const WLROOTS_NAMES: [&str; 6] = ["wlroots", "labwc", "sway", "river", "niri", "wayfire"];

/// The **running compositor** decides, not the desktop's name for
/// itself. Budgie is why: its Wayland session is labwc underneath and
/// calls itself `Budgie`, so a name check missed it — and the app
/// switched a layout that never moved, deleted the user's word and
/// retyped it identically. Measured 2026-08-24.
fn session_is_wlroots() -> bool {
    let named = crate::linux::cinnamon::DESKTOP_VARS
        .iter()
        .any(|var| std::env::var(var).is_ok_and(|value| value.split(':').any(is_wlroots_name)));
    named || wlroots_compositor_running()
}

fn is_wlroots_name(entry: &str) -> bool {
    names_any_of(entry, &WLROOTS_NAMES)
}

fn names_any_of(entry: &str, known: &[&str]) -> bool {
    let entry = entry.trim().rsplit('/').next().unwrap_or_default();
    known.iter().any(|k| entry.eq_ignore_ascii_case(k))
}

/// MATE, by any of the names a display manager might announce it with.
pub(crate) fn session_is_mate() -> bool {
    session_is_named(&["mate"])
}

/// Does any of the desktop variables name one of these? Entries are
/// matched whole, so `mate` never claims `mate-something`.
fn session_is_named(known: &[&str]) -> bool {
    crate::linux::cinnamon::DESKTOP_VARS.iter().any(|var| {
        std::env::var(var).is_ok_and(|value| value.split(':').any(|e| names_any_of(e, known)))
    })
}

/// One scan of `/proc/*/comm`, once at start-up, for a compositor of
/// our own user. Cheaper than the `gsettings` call it guards.
fn wlroots_compositor_running() -> bool {
    use std::os::unix::fs::MetadataExt;

    let Ok(entries) = std::fs::read_dir("/proc") else {
        return false;
    };
    // Our own uid, without a libc dependency for one call.
    let Ok(uid) = std::fs::metadata("/proc/self").map(|m| m.uid()) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.file_name().is_some_and(|n| {
            n.to_str()
                .is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit()))
        }) {
            continue;
        }
        // Another user's compositor says nothing about our session.
        if let Ok(meta) = std::fs::metadata(&path)
            && meta.uid() != uid
        {
            continue;
        }
        if let Ok(comm) = std::fs::read_to_string(path.join("comm"))
            && is_wlroots_name(comm.trim())
        {
            debug!(compositor = comm.trim(), "wlroots compositor detected");
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The name half of the check, kept pure: `session_is_wlroots` also
    /// scans `/proc`, so asserting on it would pass or fail depending
    /// on what the machine running the tests happens to be.
    ///
    /// `XDG_CURRENT_DESKTOP` on the guest reads `labwc:wlroots`, so
    /// either entry has to be enough — and entries are matched whole,
    /// because `swaything` is a fork whose input stack nobody has seen.
    #[test]
    fn wlroots_sessions_are_recognised_by_either_half_of_the_name() {
        for value in [
            "labwc", "wlroots", "sway", "SWAY", "river", "niri", "wayfire",
        ] {
            assert!(is_wlroots_name(value), "{value} should read as wlroots");
        }
        for value in ["GNOME", "ubuntu", "KDE", "swaything", "Budgie", ""] {
            assert!(
                !is_wlroots_name(value),
                "{value} should not read as wlroots"
            );
        }
        // Display managers write whole paths into these variables.
        assert!(is_wlroots_name("/usr/share/wayland-sessions/sway"));
    }

    /// The list that decides whether this backend claims a session at
    /// all. Kept pure for the same reason as the one above: reading the
    /// environment would assert on whatever desktop runs the suite.
    #[test]
    fn only_the_desktops_that_act_on_the_schema_are_named() {
        let names = |value: &str| {
            value
                .split(':')
                .any(|e| names_any_of(e, &GNOME_FAMILY_NAMES))
        };

        // Ubuntu's GNOME announces itself with two entries; either is
        // enough.
        for value in [
            "GNOME",
            "ubuntu:GNOME",
            "gnome",
            "GNOME-Flashback:GNOME",
            "Unity",
            "Budgie:GNOME",
            "Pantheon",
        ] {
            assert!(names(value), "{value} should read as GNOME-family");
        }
        // The six that took this backend on a machine whose dconf had
        // been populated by an earlier GNOME session, and could not
        // switch a thing (2026-08-27), plus the two that had a branch
        // of their own before.
        for value in [
            "i3",
            "ICEWM",
            "LXQt",
            "XFCE",
            "",
            "X-Cinnamon",
            "MATE",
            "sway:wlroots",
        ] {
            assert!(!names(value), "{value} must not claim this backend");
        }
    }
}
