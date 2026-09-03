//! `PluginPane` itself: state fields and construction. Behaviour lives
//! in the sibling files, one `impl` block per concern.

use std::path::{Path, PathBuf};

use poltertype_core::plugins::{
    ControlKind, DiscoveredExtension, PaneControl, SettingValue, read_setting, read_string_array,
};

use super::enums::CommandOutput;
use super::types::{ListRow, RecordRow, Slot};

/// One plug-in as the pane sees it.
pub struct PluginPane {
    pub ext: DiscoveredExtension,
    /// The plug-in's own config file. May not exist yet — a plug-in is
    /// allowed to run entirely on its defaults.
    pub config_path: PathBuf,
    /// Current value per control, positionally. `None` means the file
    /// does not set it, so the plug-in's own default applies and we
    /// must not pretend to know what it is.
    pub values: Vec<Option<SettingValue>>,
    /// Result of the last edit, shown next to the plug-in.
    pub status: Option<String>,
    /// What each command-backed box is showing. A map rather than a
    /// vector of defaults because absent means "never asked", which is
    /// different from "asked, empty" — only one of them sends a command.
    ///
    /// Private, and written only through [`super::commands`], so the
    /// rows parsed out of it cannot be left describing an older answer.
    pub(super) outputs: std::collections::HashMap<Slot, CommandOutput>,
    /// Which section is on screen, as a control index; `None` means the
    /// first one. One section at a time rather than an accordion:
    /// thirteen fold arrows over a page that is still metres long is not
    /// navigation.
    pub section: Option<usize>,
    /// What is in a text box right now, before it is a value.
    ///
    /// Without this the box can only show what the *file* holds, so a
    /// number cannot be cleared and a decimal cannot be typed — "0." is
    /// not a number, is therefore not written, and the box snaps back
    /// before the next character arrives.
    pub edits: std::collections::HashMap<usize, String>,
    /// Which members each list control's array currently holds, by
    /// control index — what decides whether a row's box is ticked.
    ///
    /// Cached because re-reading per *row* costs a `read_to_string` plus
    /// a whole format-preserving TOML parse each, measured at 78 µs
    /// against a 17 KB config: two room lists of 34 conversations read
    /// **1.2 MB and ran 68 TOML parses for every click**, since `view`
    /// rebuilds on every state change. Refreshed wherever the file can
    /// have changed — see [`super::arrays`].
    pub(super) arrays: std::collections::HashMap<usize, Vec<String>>,
    /// The rows a command-backed box is drawing, parsed once when the
    /// plug-in's answer arrives rather than re-split on every rebuild.
    pub(super) rows: std::collections::HashMap<Slot, Vec<ListRow>>,
    /// Which suggestion box has its list open, if any. One at a time,
    /// and inline: iced's own combo box draws an overlay sized to its
    /// options, which ninety-five conversations turned into a modal over
    /// the whole form.
    pub(super) open_suggest: Option<Slot>,
    /// Which card's button is running. A row action takes seconds and
    /// changes the world, and a button that goes quiet for twenty
    /// seconds reads as one that did nothing.
    pub(super) running_action: Option<(usize, usize)>,
    /// What each repeating-group control holds, by control index: one
    /// entry per row, each mapping the declared field names to what the
    /// file says. Cached for the same reason `arrays` is — reading a
    /// field at a time is a format-preserving TOML parse per field per
    /// row, on every rebuild.
    pub(super) records: std::collections::HashMap<usize, Vec<RecordRow>>,
    /// What is being typed into a record's field, before it is a value —
    /// the per-row counterpart of `edits`: saving per keystroke would
    /// put every prefix of a message into a file the plug-in reads.
    pub(super) record_edits: std::collections::HashMap<(usize, usize, String), String>,
}

impl PluginPane {
    /// Read the current values for one extension.
    ///
    /// `config_root` is the directory holding *per-application* config
    /// directories — the parent of ours. A plug-in is a separate
    /// program, so its config sits beside PolterType's, not inside it.
    pub fn load(ext: DiscoveredExtension, config_root: &Path) -> Self {
        let config_path = config_root
            .join(&ext.id)
            .join(if ext.manifest.config_file.is_empty() {
                "config.toml"
            } else {
                &ext.manifest.config_file
            });
        let text = std::fs::read_to_string(&config_path).unwrap_or_default();
        let values = ext
            .manifest
            .pane
            .iter()
            .map(|c| {
                if c.key.is_empty() {
                    None
                } else if c.kind == ControlKind::Strings {
                    // An array has no `SettingValue`, and it does not
                    // need one: the box shows the members joined, and
                    // what is written back is always a fresh array.
                    let members = read_string_array(&text, &c.key);
                    (!members.is_empty()).then(|| SettingValue::Text(members.join(", ")))
                } else {
                    read_setting(&text, &c.key)
                }
            })
            .collect();
        let mut pane = Self {
            ext,
            config_path,
            values,
            status: None,
            outputs: std::collections::HashMap::new(),
            section: None,
            edits: std::collections::HashMap::new(),
            arrays: std::collections::HashMap::new(),
            rows: std::collections::HashMap::new(),
            open_suggest: None,
            records: std::collections::HashMap::new(),
            record_edits: std::collections::HashMap::new(),
            running_action: None,
        };
        pane.reload_arrays();
        pane.reload_records();
        pane
    }

    pub fn control(&self, index: usize) -> Option<&PaneControl> {
        self.ext.manifest.pane.get(index)
    }
}
