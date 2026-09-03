//! The plug-in command API: running a declared command, and reading
//! back what a plug-in reports.

use std::collections::HashMap;

use poltertype_core::plugins::DiscoveredExtension;
use tracing::{info, warn};

use crate::plugins::consts::{ROW_ID_PLACEHOLDER, STOP_COMMAND};
use crate::plugins::menu::parse_rows;
use crate::plugins::types::MenuRow;

use super::consts::{ACTION_TIMEOUT, REPORT_TIMEOUT, STATE_TIMEOUT};
use super::process::{capture_output, spawn};

/// Whether this extension declared the reserved stop command.
pub(super) fn declares_stop(ext: &DiscoveredExtension) -> bool {
    ext.manifest.commands.iter().any(|c| c.id == STOP_COMMAND)
}

/// Run one of a plug-in's declared commands and leave it to finish on
/// its own. Used by tray entries and pane buttons, which must return
/// immediately — the menu is on the UI thread.
pub fn run_command(ext: &DiscoveredExtension, command_id: &str) -> Result<(), String> {
    let cmd = ext
        .manifest
        .commands
        .iter()
        .find(|c| c.id == command_id)
        .ok_or_else(|| format!("{} declares no command {command_id:?}", ext.id))?;

    // No log file for a one-shot: it inherits. The service log exists
    // because a service dies unobserved.
    spawn(&ext.exe, &cmd.args, &ext.dir, None)
        .map(|child| {
            info!(id = %ext.id, command = %command_id, pid = child.id(), "plug-in command started");
            // The child is reparented when it outlives us, which for a
            // one-shot command is the right outcome.
        })
        .map_err(|e| format!("could not run {command_id:?}: {e}"))
}

/// Run a declared command with one row id substituted into it — what a
/// per-row tray entry does.
///
/// Substitution is by whole argument: an argument that *is* `{id}`
/// becomes the row id, and nothing else is touched. Not string
/// interpolation, deliberately — a row id arrives from the plug-in's own
/// output, and pasting it into the middle of an argument is how it would
/// turn into a second flag.
pub fn run_command_for_row(
    ext: &DiscoveredExtension,
    command_id: &str,
    row_id: &str,
) -> Result<(), String> {
    let cmd = ext
        .manifest
        .commands
        .iter()
        .find(|c| c.id == command_id)
        .ok_or_else(|| format!("{} declares no command {command_id:?}", ext.id))?;
    let args: Vec<String> = cmd
        .args
        .iter()
        .map(|a| {
            if a == ROW_ID_PLACEHOLDER {
                row_id.to_owned()
            } else {
                a.clone()
            }
        })
        .collect();

    spawn(&ext.exe, &args, &ext.dir, None)
        .map(|child| {
            info!(id = %ext.id, command = %command_id, row = %row_id, pid = child.id(), "plug-in row command started");
        })
        .map_err(|e| format!("could not run {command_id:?}: {e}"))
}

/// The same substitution as [`run_command_for_row`], but waiting for the
/// answer — what a pane's own row button needs.
///
/// A tray entry can afford to fire and forget; a button on a card cannot
/// — "Send this message now" with no visible outcome leaves the one
/// question it was pressed to answer unanswered.
///
/// So this waits, off the UI thread, with `ACTION_TIMEOUT` rather than
/// a report's six seconds: the action behind such a button is not a
/// query. Sending a standing message opens a chat client, waits for the
/// conversation to switch and types a sentence at human speed.
pub fn run_command_for_row_waiting(
    ext: &DiscoveredExtension,
    command_id: &str,
    row_id: &str,
) -> Result<String, String> {
    let cmd = ext
        .manifest
        .commands
        .iter()
        .find(|c| c.id == command_id)
        .ok_or_else(|| format!("{} declares no command {command_id:?}", ext.id))?;
    let args: Vec<String> = cmd
        .args
        .iter()
        .map(|a| {
            if a == ROW_ID_PLACEHOLDER {
                row_id.to_owned()
            } else {
                a.clone()
            }
        })
        .collect();
    info!(id = %ext.id, command = %command_id, row = %row_id, "plug-in row action running");
    capture_output(ext, &args, ACTION_TIMEOUT, "row action")
}

/// Ask a plug-in for the rows of one of its runtime menus.
///
/// Waited on with a deadline, like the state read and for the same
/// reason: this runs while a menu is being built. An empty answer and a
/// failed one are both "no rows" here — the menu says so either way, and
/// the log carries the difference.
pub fn read_rows(ext: &DiscoveredExtension, command_id: &str) -> Vec<MenuRow> {
    let Some(cmd) = ext.manifest.commands.iter().find(|c| c.id == command_id) else {
        warn!(id = %ext.id, "no command {command_id:?} to list from");
        return Vec::new();
    };
    match capture_output(ext, &cmd.args, STATE_TIMEOUT, "list command") {
        Ok(out) => parse_rows(&out),
        Err(e) => {
            warn!(id = %ext.id, "cannot read plug-in menu rows: {e}");
            Vec::new()
        }
    }
}

/// Ask a plug-in what state it is in, for the tray to reflect.
///
/// Unlike [`run_command`] this **is** waited on, so it carries a
/// deadline — a plug-in that hangs here would freeze the tray menu.
///
/// Output is one `key=value` per line; anything else is ignored rather
/// than rejected, so a plug-in may print a human-facing summary too.
///
/// `None` means the plug-in could not be asked at all, which the menu
/// renders differently from an answer that omits a key: one is worth
/// investigating and the other is normal.
pub fn read_state(ext: &DiscoveredExtension) -> Option<HashMap<String, String>> {
    if ext.manifest.state_args.is_empty() {
        return None;
    }

    let stdout = match state_output(ext) {
        Ok(out) => out,
        Err(e) => {
            warn!(id = %ext.id, "cannot read plug-in state: {e}");
            return None;
        }
    };

    let mut state = HashMap::new();
    for line in stdout.lines() {
        if let Some((key, value)) = line.split_once('=') {
            let (key, value) = (key.trim(), value.trim());
            if !key.is_empty() {
                state.insert(key.to_owned(), value.to_owned());
            }
        }
    }
    Some(state)
}

/// Run the state command and return its stdout, or give up.
///
/// The deadline is why this is not one `output()` call: that waits for
/// ever, and this runs on the UI thread while a menu is drawn. A stale
/// tick beats a tray that stops responding.
fn state_output(ext: &DiscoveredExtension) -> Result<String, String> {
    capture_output(
        ext,
        &ext.manifest.state_args,
        STATE_TIMEOUT,
        "state command",
    )
}

/// Run one of a plug-in's declared commands and return what it printed.
///
/// Separate from [`run_command`] rather than a flag on it: that one must
/// return before the child does, because it runs on the thread drawing a
/// menu. This one backs a pane *showing* an answer, so it waits.
pub fn read_report(ext: &DiscoveredExtension, command_id: &str) -> Result<String, String> {
    let cmd = ext
        .manifest
        .commands
        .iter()
        .find(|c| c.id == command_id)
        .ok_or_else(|| format!("{} declares no command {command_id:?}", ext.id))?;
    capture_output(ext, &cmd.args, REPORT_TIMEOUT, "report command")
}
