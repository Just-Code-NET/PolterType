//! Free functions: parsing a command's output into rows, and loading
//! every plug-in that declares a pane.

use std::path::Path;

use poltertype_core::plugins::DiscoveredExtension;

use super::pane::PluginPane;
use super::types::ListRow;

/// Parse a list command's output into rows.
///
/// Tab-separated and tolerant in the same way the state protocol is —
/// a line with no tab is an id that is its own label, extra fields are
/// ignored, blank lines skipped. A plug-in should be able to print
/// something readable without it becoming a parsing contract.
pub(super) fn parse_list_rows(text: &str) -> Vec<ListRow> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let mut fields = line.split('\t');
            let id = fields.next().unwrap_or_default().trim().to_owned();
            let label = fields.next().unwrap_or_default().trim();
            let detail = fields.next().unwrap_or_default().trim();
            ListRow {
                label: if label.is_empty() {
                    id.clone()
                } else {
                    label.to_owned()
                },
                id,
                detail: detail.to_owned(),
            }
        })
        .filter(|row| !row.id.is_empty())
        .collect()
}

/// Load every discovered extension that actually declares a pane.
///
/// A plug-in with no controls gets no section: an empty box with a
/// name in it tells the user nothing and makes the list longer.
pub fn load_all(extensions: Vec<DiscoveredExtension>, config_root: &Path) -> Vec<PluginPane> {
    extensions
        .into_iter()
        .filter(|e| !e.manifest.pane.is_empty())
        .map(|e| PluginPane::load(e, config_root))
        .collect()
}
