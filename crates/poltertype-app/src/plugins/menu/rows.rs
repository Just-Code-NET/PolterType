//! Turning a plug-in's list-command output into menu rows.

use poltertype_core::i18n::tr_args;
use poltertype_core::plugins::TrayList;

use crate::plugins::consts::LIST_INLINE_ROWS;
use crate::plugins::types::MenuRow;

use super::enums::ListEntry;

/// Parse a list command's output into rows: `id`, label, then any number
/// of detail lines, tab-separated.
///
/// The same shape the settings pane's tick-box lists use, and tolerant in
/// the same way: a line with no tab is an id that is its own label, blank
/// lines are skipped, and a row with no id is dropped because there would
/// be nothing to act on.
pub fn parse_rows(text: &str) -> Vec<MenuRow> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let mut fields = line.split('\t');
            let id = fields.next().unwrap_or_default().trim().to_owned();
            let label = fields.next().unwrap_or_default().trim().to_owned();
            let details = fields
                .map(str::trim)
                .filter(|d| !d.is_empty())
                .map(str::to_owned)
                .collect();
            MenuRow {
                label: if label.is_empty() { id.clone() } else { label },
                id,
                details,
            }
        })
        .filter(|row| !row.id.is_empty())
        .collect()
}

/// Lay a list out as one flat submenu: each row's words, then its actions,
/// groups split by a line.
///
/// Flat because a menu that opens a further menu per row closes on any
/// hover that strays across a neighbour on the way there, and the pointer
/// is sent back to the start. With a single level there is no path to
/// fall off: every line that can be clicked is already on screen.
///
/// Flat also means tall, so only [`LIST_INLINE_ROWS`] are drawn and the rest
/// are counted. Dealing with one brings the next into view.
pub(super) fn list_entries(rows: &[MenuRow], spec: &TrayList) -> Vec<ListEntry> {
    let mut entries = Vec::new();
    for (index, row) in rows.iter().take(LIST_INLINE_ROWS).enumerate() {
        if index > 0 {
            entries.push(ListEntry::Separator);
        }
        entries.push(ListEntry::Note(row.label.clone()));
        entries.extend(row.details.iter().cloned().map(ListEntry::Note));
        entries.extend(spec.actions.iter().map(|action| ListEntry::Action {
            label: action.label.clone(),
            command: action.command.clone(),
            row: row.id.clone(),
        }));
    }
    if rows.len() > LIST_INLINE_ROWS {
        entries.push(ListEntry::Separator);
        entries.push(ListEntry::Note(tr_args(
            "plugins.menu_more",
            "…and {} more — they move up as you clear these",
            &[&(rows.len() - LIST_INLINE_ROWS).to_string()],
        )));
    }
    if !spec.bulk.is_empty() {
        entries.push(ListEntry::Separator);
        entries.extend(spec.bulk.iter().map(|action| ListEntry::Action {
            label: action.label.clone(),
            command: action.command.clone(),
            row: String::new(),
        }));
    }
    entries
}
