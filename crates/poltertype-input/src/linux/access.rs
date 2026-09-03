//! Who may open `/dev/input` here — and when nobody may, which of the
//! four different fixes actually applies.
//!
//! Shared by the evdev listener's failure message and the Setup pane so
//! the two can never tell the user different stories. Split out after
//! issue #31, where someone who had run `setup-linux.sh` twice was told
//! by the app to run `setup-linux.sh`.

use std::path::PathBuf;

mod consts;
mod types;

use consts::INPUT_GROUP;
pub(crate) use consts::{EVENT_DEVICE_DIR, PERMISSIONS_URL};
pub(crate) use types::{NodeFacts, ScanFacts};

/// The command we hand the user — pointing at a script they actually
/// have. An AppImage carries its own copy, and naming a repository path
/// to someone who downloaded a single file is advice they cannot act
/// on. Never run for them: the script needs `sudo`, and an app that
/// quietly acquires root has spent trust it will not get back.
pub(crate) fn setup_script_command() -> String {
    bundled_setup_script().map_or_else(
        || "bash scripts/setup-linux.sh".to_owned(),
        |p| format!("bash {}", p.display()),
    )
}

/// The AppImage lays itself out FHS-shaped, so the script sits beside
/// the bundled data at `<exe_dir>/../share/poltertype/scripts`.
fn bundled_setup_script() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let path = exe
        .parent()?
        .parent()?
        .join("share/poltertype/scripts/setup-linux.sh");
    path.is_file().then_some(path)
}

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
             devices will open ({why}) — run `{}`, then log out and back in. What it does, \
             and how to do it by hand: {PERMISSIONS_URL}",
            setup_script_command()
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
