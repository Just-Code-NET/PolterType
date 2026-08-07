//! State behind the Plug-ins pane: one entry per installed extension,
//! holding the values its manifest declared and knowing how to write
//! them back.
//!
//! The pane edits *the plug-in's* config file, not ours. That file is
//! written and read by a program we did not write, so two rules apply
//! throughout: only the keys the manifest declared are ever touched,
//! and a write that cannot be made cleanly is reported rather than
//! forced. Everything else in the file — including every comment its
//! author left for the user — comes back unchanged, which is
//! [`poltertype_core::plugins::write_setting`]'s whole job.

use std::path::{Path, PathBuf};

use poltertype_core::plugins::{
    ControlKind, DiscoveredExtension, PaneControl, SettingValue, read_setting, write_setting,
};
use tracing::warn;

/// What a report control is showing right now.
///
/// Three states and not two: "asked, waiting" reads very differently
/// from "asked, got nothing", and a pane that shows an empty box for
/// both is a pane that looks broken while it is working.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportState {
    /// The command is running.
    Loading,
    /// It answered. May legitimately be empty text.
    Ready(String),
    /// It could not be asked, or it failed.
    Failed(String),
}

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
    /// What each report control is showing, by control index. Absent
    /// means it has not been asked yet — which is why it is a map and
    /// not a vector of defaults: "never asked" and "asked, empty" are
    /// different, and only one of them should send a command.
    pub reports: std::collections::HashMap<usize, ReportState>,
}

impl PluginPane {
    /// Which controls are reports that have not been asked yet.
    ///
    /// The pane asks on the way in rather than on every draw: a report
    /// costs a process, and `view` runs on every frame.
    pub fn unasked_reports(&self) -> Vec<usize> {
        self.ext
            .manifest
            .pane
            .iter()
            .enumerate()
            .filter(|(i, c)| c.kind == ControlKind::Report && !self.reports.contains_key(i))
            .map(|(i, _)| i)
            .collect()
    }

    /// Read the current values for one extension.
    ///
    /// `config_root` is the directory holding *per-application* config
    /// directories — the parent of ours — because a plug-in keeps its
    /// config beside PolterType's rather than inside it. It is a
    /// separate program; its settings are not a subsection of ours.
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
                } else {
                    read_setting(&text, &c.key)
                }
            })
            .collect();
        Self {
            ext,
            config_path,
            values,
            status: None,
            reports: std::collections::HashMap::new(),
        }
    }

    /// The value to render for a control: what the file says, or the
    /// neutral default for its kind.
    pub fn value_of(&self, index: usize) -> SettingValue {
        match self.values.get(index).and_then(|v| v.clone()) {
            Some(v) => v,
            None => match self.ext.manifest.pane.get(index).map(|c| c.kind) {
                Some(ControlKind::Toggle) => SettingValue::Bool(false),
                Some(ControlKind::Number) => SettingValue::Int(0),
                _ => SettingValue::Text(String::new()),
            },
        }
    }

    pub fn control(&self, index: usize) -> Option<&PaneControl> {
        self.ext.manifest.pane.get(index)
    }

    /// Write one control's value into the plug-in's config file.
    ///
    /// Reads, edits and writes on the spot rather than batching: the
    /// plug-in may be running and watching that file, and a pane that
    /// held changes back would be showing a state the plug-in is not
    /// in.
    pub fn set(&mut self, index: usize, value: SettingValue) {
        let Some(control) = self.ext.manifest.pane.get(index) else {
            return;
        };
        if control.key.is_empty() {
            return;
        }
        let key = control.key.clone();

        let current = std::fs::read_to_string(&self.config_path).unwrap_or_default();
        match write_setting(&current, &key, &value) {
            Ok(updated) => {
                if let Some(dir) = self.config_path.parent() {
                    if let Err(e) = std::fs::create_dir_all(dir) {
                        self.status = Some(format!("Could not create {}: {e}", dir.display()));
                        return;
                    }
                }
                match std::fs::write(&self.config_path, updated) {
                    Ok(()) => {
                        self.values[index] = Some(value);
                        self.status = Some(format!("Saved to {}", self.config_path.display()));
                    }
                    Err(e) => {
                        warn!(path = %self.config_path.display(), "cannot write plug-in config: {e}");
                        self.status = Some(format!(
                            "Could not write {}: {e}",
                            self.config_path.display()
                        ));
                    }
                }
            }
            Err(e) => {
                // The plug-in's file is not something we may rewrite on
                // a guess — say what is wrong and change nothing.
                self.status = Some(format!("{e}"));
            }
        }
    }
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

#[cfg(test)]
#[path = "plugin_pane_tests.rs"]
mod tests;
