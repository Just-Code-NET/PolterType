//! Turning a plug-in's list-command output into menu rows.

use crate::plugins::types::MenuRow;

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
