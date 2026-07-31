//! What a Linux user has to grant, and whether they have.
//!
//! Only Wayland sessions have anything to grant. X11 needs no
//! permissions at all — XInput2 and XTest are available to any client
//! that can open the display — and saying so is part of the job: most
//! people arrive expecting the worst.

use std::path::Path;

use super::consts::{
    EVENT_DEVICE_DIR, INPUT_GROUP, PERMISSIONS_URL, SETUP_SCRIPT_COMMAND, UINPUT_DEVICE,
};
use super::enums::{StepAction, StepState};
use super::types::{SetupReport, SetupStep};
use crate::linux::{SessionKind, session_kind};

pub(super) fn probe() -> SetupReport {
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

/// Turn "does this capability work" plus "what does the group database
/// say" into a state and the one useful next action.
fn step(title: &str, detail: &str, works: Option<bool>, group: GroupState) -> SetupStep {
    let (state, action) = match (works, group) {
        (Some(true), _) => (StepState::Done, None),
        // The trap this whole state exists for: `usermod -aG input`
        // updated the group database and cannot touch the credentials
        // of an already-running session. Everything looks configured,
        // nothing works, and re-running the script changes nothing —
        // so telling the user to re-run it would waste their evening.
        // No button: the fix is "log out", which we cannot do for
        // them and a link cannot explain better than the one sentence
        // the pane prints once for every step in this state.
        (Some(false), GroupState::InDatabaseOnly) => (StepState::NeedsRelogin, None),
        (Some(false), _) => (
            StepState::Todo,
            Some(StepAction::Copy(SETUP_SCRIPT_COMMAND.to_owned())),
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

/// How the `input` group looks from the two places that disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroupState {
    /// The group database lists us *and* this session carries the gid.
    Active,
    /// Listed in `/etc/group`, absent from this session's credentials
    /// — the log-out-and-back-in case.
    InDatabaseOnly,
    /// Not a member anywhere, or we could not tell.
    Absent,
}

fn group_state() -> GroupState {
    let Some(gid) = input_group_gid() else {
        return GroupState::Absent;
    };
    // Safety: `getgid` takes no arguments and cannot fail.
    let primary = unsafe { libc::getgid() };
    if primary == gid || session_groups().is_some_and(|gs| gs.contains(&gid)) {
        return GroupState::Active;
    }
    if user_listed_in_input_group() {
        return GroupState::InDatabaseOnly;
    }
    GroupState::Absent
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

/// gid of the `input` group from the group database.
fn input_group_gid() -> Option<u32> {
    for line in std::fs::read_to_string("/etc/group").ok()?.lines() {
        let mut parts = line.split(':');
        if parts.next()? != INPUT_GROUP {
            continue;
        }
        let _passwd = parts.next()?;
        return parts.next()?.parse().ok();
    }
    None
}

/// Is our user name in the `input` group's member list?
///
/// Read from `/etc/group` rather than resolved through NSS: this is a
/// diagnostic, and the case it exists to catch — `usermod` has run,
/// the session predates it — is exactly a local-file edit.
fn user_listed_in_input_group() -> bool {
    let Some(user) = current_user_name() else {
        return false;
    };
    let Ok(text) = std::fs::read_to_string("/etc/group") else {
        return false;
    };
    text.lines()
        .filter(|l| l.starts_with(concat!("input", ":")))
        .any(|l| {
            l.rsplit(':')
                .next()
                .is_some_and(|members| members.split(',').any(|m| m == user))
        })
}

fn current_user_name() -> Option<String> {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .ok()
        .filter(|u| !u.is_empty())
}

/// The supplementary groups *this process* actually carries — the
/// half of the comparison that a `usermod` cannot change.
fn session_groups() -> Option<Vec<u32>> {
    // Safety: the two-call form documented in getgroups(2). The first
    // call (size 0) only reports how many there are and writes
    // nothing, so the null pointer is what the manual asks for.
    let count = unsafe { libc::getgroups(0, std::ptr::null_mut()) };
    if count < 0 {
        return None;
    }
    let mut buf = vec![0 as libc::gid_t; count as usize];
    // Safety: `buf` has room for exactly `count` gids, which is what
    // we tell the kernel.
    let filled = unsafe { libc::getgroups(count, buf.as_mut_ptr()) };
    if filled < 0 {
        return None;
    }
    buf.truncate(filled as usize);
    Some(buf)
}
