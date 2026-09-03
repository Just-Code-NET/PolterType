//! What a Linux user has to grant, and whether they have.
//!
//! Only Wayland sessions have anything to grant. X11 needs no
//! permissions at all — XInput2 and XTest are available to any client
//! that can open the display — and saying so is part of the job: most
//! people arrive expecting the worst.

use std::path::Path;

use super::enums::{Permission, StepAction, StepState};
use super::types::{SetupReport, SetupStep};
use crate::linux::access::{
    EVENT_DEVICE_DIR, GroupState, PERMISSIONS_URL, group_state, setup_script_command,
};
use crate::linux::{SessionKind, session_kind};

/// Where `create_key_gate`'s evdev backend opens its virtual keyboard.
const UINPUT_DEVICE: &str = "/dev/uinput";

pub(super) fn probe(_local_signing_identity: &str) -> SetupReport {
    match session_kind() {
        SessionKind::X11 => SetupReport {
            backend: Some("linux-x11-xinput2".to_owned()),
            steps: vec![SetupStep {
                title: "Nothing to set up on X11".to_owned(),
                detail: "X11 hands global key events to any client that can open the display, \
                         so PolterType needs no group membership, no udev rule and no sudo here."
                    .to_owned(),
                state: StepState::Done,
                action: None,
            }],
        },
        SessionKind::Wayland | SessionKind::Unknown => wayland_report(),
    }
}

/// No system dialog exists here — Linux permissions are group
/// membership and udev rules, granted with the copyable command the
/// probe above already offers.
pub(super) fn request(_permission: Permission) -> bool {
    false
}

pub(super) fn settings_pane_url(_permission: Permission) -> Option<&'static str> {
    None
}

pub(super) fn setup_local_signing(_name: &str) -> Result<(), String> {
    Err("local update signing is a macOS mechanism".to_owned())
}

fn wayland_report() -> SetupReport {
    // The two capabilities, probed independently: reading the keyboard
    // and typing the correction are separate permissions and fail
    // separately. A user with read access but no uinput sees detection
    // work and nothing get fixed, which is the more confusing half.
    let read = any_readable_event_device(Path::new(EVENT_DEVICE_DIR));
    let write = writable(Path::new(UINPUT_DEVICE));

    // Only consulted when something is actually wrong: when both work,
    // *how* the user got there is none of our business.
    let group = group_state();

    SetupReport {
        backend: Some("linux-wayland-evdev".to_owned()),
        steps: vec![
            step(
                "Read the keyboard",
                "PolterType watches key events straight from /dev/input, because Wayland \
                 deliberately offers no way for one app to see another's keystrokes. \
                 Read access only — nothing is written back to those devices.",
                read,
                group,
            ),
            step(
                "Type the correction",
                "Fixing a word means synthesising backspaces and letters through /dev/uinput, \
                 a virtual keyboard the kernel creates for us. Without it PolterType can spot \
                 the wrong layout but not repair it.",
                write,
                group,
            ),
        ],
    }
}

fn step(title: &str, detail: &str, works: Option<bool>, group: GroupState) -> SetupStep {
    let (state, action) = match (works, group) {
        (Some(true), _) => (StepState::Done, None),
        // The trap this state exists for: `usermod -aG input` updates
        // the group database and cannot touch the credentials of an
        // already-running session. Everything looks configured, nothing
        // works, and re-running the script changes nothing. No button —
        // the fix is "log out", which we cannot do for them.
        (Some(false), GroupState::InDatabaseOnly) => (StepState::NeedsRelogin, None),
        (Some(false), _) => (
            StepState::Todo,
            Some(StepAction::Copy(setup_script_command())),
        ),
        (None, _) => (
            StepState::Unknown,
            Some(StepAction::Open(PERMISSIONS_URL.to_owned())),
        ),
    };
    SetupStep {
        title: title.to_owned(),
        detail: detail.to_owned(),
        state,
        action,
    }
}

// ─── The probes themselves ────────────────────────────────────────────

/// `None` throughout this section means "could not tell" — a missing
/// directory, a read error. Never guessed at: a setup guide that
/// invents a problem is worse than one that admits ignorance.
fn any_readable_event_device(dir: &Path) -> Option<bool> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut saw_one = false;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("event"))
        {
            continue;
        }
        saw_one = true;
        if readable(&path) {
            return Some(true);
        }
    }
    // Devices exist and not one of them opens: a real, reportable no.
    // No devices at all (a container, an odd kernel) is not something
    // we can turn into advice.
    saw_one.then_some(false)
}

fn readable(path: &Path) -> bool {
    std::fs::File::open(path).is_ok()
}

/// A missing `/dev/uinput` counts as "not yet" rather than "cannot
/// tell": it means the kernel module is not loaded, which
/// `setup-linux.sh` also fixes, so the advice is the same.
fn writable(path: &Path) -> Option<bool> {
    if !path.exists() {
        return Some(false);
    }
    Some(std::fs::OpenOptions::new().write(true).open(path).is_ok())
}
