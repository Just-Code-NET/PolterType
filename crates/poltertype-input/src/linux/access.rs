//! Who may open `/dev/input` here — and when nobody may, which of the
//! four different fixes actually applies.
//!
//! Shared by the evdev listener's failure message and the Setup pane so
//! the two can never tell the user different stories. Split out after
//! issue #31, where someone who had run `setup-linux.sh` twice was told
//! by the app to run `setup-linux.sh`.

use std::os::unix::fs::MetadataExt;
use std::path::Path;

pub(crate) const EVENT_DEVICE_DIR: &str = "/dev/input";

pub(crate) const INPUT_GROUP: &str = "input";

/// The permissions guide, pinned to `main` for the same reason the
/// tray's link is: it has to describe the current setup script, not the
/// release the user happens to be running.
pub(crate) const PERMISSIONS_URL: &str =
    "https://github.com/Just-Code-NET/PolterType/blob/main/docs/PERMISSIONS.md";

/// How the `input` group looks from the two places that disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GroupState {
    /// The group database lists us *and* this session carries the gid.
    Active,
    /// Listed in `/etc/group`, absent from this session's credentials
    /// — the log-out-and-back-in case.
    InDatabaseOnly,
    /// Not a member anywhere, or we could not tell.
    Absent,
}

pub(crate) fn group_state() -> GroupState {
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

/// What one scan of `/dev/input` found. Counts rather than devices:
/// this is everything the failure message is built from, which is what
/// lets a unit test drive every branch of it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct ScanFacts {
    /// `None` when `/dev/input` itself could not be listed.
    pub(crate) nodes: Option<usize>,
    pub(crate) opened: usize,
    /// Of the opened ones, how many advertise `KEY_A`.
    pub(crate) keyboards: usize,
    /// Verbatim errno text of the first thing that refused us.
    pub(crate) first_error: Option<String>,
    /// The node that produced `first_error`, as the kernel presents it.
    pub(crate) sample: Option<NodeFacts>,
}

/// Ownership of one device node, in the numeric form `stat` reports.
/// Numeric on purpose: resolving names needs NSS, and the whole point
/// of printing this is that the naming layer may be what is lying.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NodeFacts {
    pub(crate) name: String,
    pub(crate) uid: u32,
    pub(crate) gid: u32,
    pub(crate) mode: u32,
}

impl NodeFacts {
    pub(crate) fn of(path: &Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        Some(Self {
            name: path.display().to_string(),
            uid: meta.uid(),
            gid: meta.gid(),
            mode: meta.mode() & 0o7777,
        })
    }
}

impl std::fmt::Display for NodeFacts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} is uid={} gid={} mode={:04o}",
            self.name, self.uid, self.gid, self.mode
        )
    }
}

/// The sentence the user sees when no keyboard could be opened.
///
/// Every branch names a *different* action, because the failure has
/// four unrelated causes that a single "run the setup script" hides —
/// and the script is a no-op for three of them.
pub(crate) fn no_keyboards_message(facts: &ScanFacts, group: GroupState) -> String {
    let Some(nodes) = facts.nodes else {
        let why = facts.first_error.as_deref().unwrap_or("unknown error");
        return format!(
            "{EVENT_DEVICE_DIR} could not be listed ({why}) — PolterType has no way to see \
             the keyboard. A container without the input devices mapped in looks like this."
        );
    };
    if nodes == 0 {
        return format!(
            "no {EVENT_DEVICE_DIR}/event* devices exist on this machine, so there is nothing \
             to read. Nothing you can grant will change that — see {PERMISSIONS_URL}"
        );
    }
    if facts.opened > 0 {
        return format!(
            "{} of {nodes} input devices opened, but none of them is a keyboard. If a remapper \
             (keyd, kmonad, Interception Tools) owns the keyboard exclusively, PolterType \
             cannot read it — see {PERMISSIONS_URL}",
            facts.opened
        );
    }
    let why = facts.first_error.as_deref().unwrap_or("permission denied");
    match group {
        // The trap this branch exists for: `usermod -aG input` updates
        // the group database and cannot touch the credentials of an
        // already-running session, so everything looks configured and
        // nothing works. Re-running the script changes nothing.
        GroupState::InDatabaseOnly => format!(
            "you are in the '{INPUT_GROUP}' group, but this login session started before that, \
             so it does not carry the group yet — log out and back in. Note that `newgrp \
             {INPUT_GROUP}` only affects the shell you typed it in; an app launched from the \
             desktop never sees it. Nothing else needs installing."
        ),
        GroupState::Absent => format!(
            "not a member of the '{INPUT_GROUP}' group, so none of the {nodes} keyboard \
             devices will open ({why}) — run `bash scripts/setup-linux.sh`, then log out and \
             back in."
        ),
        // Group is right and the devices still refuse: the udev rule
        // never reached the nodes that already existed. Printing what
        // the kernel actually says beats guessing at it.
        GroupState::Active => {
            let sample = facts
                .sample
                .as_ref()
                .map_or_else(String::new, |n| format!(" ({n})"));
            format!(
                "this session is in the '{INPUT_GROUP}' group, yet none of the {nodes} keyboard \
                 devices will open: {why}{sample}. The udev rule that grants the group read \
                 access has not been applied to these devices — re-run `bash \
                 scripts/setup-linux.sh` (it re-triggers udev) or reboot. If they are still \
                 owned by another group, something else on this system is setting it: \
                 {PERMISSIONS_URL}"
            )
        }
    }
}

// ─── The group probes themselves ──────────────────────────────────────

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

#[cfg(test)]
mod tests;
