//! What an install reports back, and what an extension declares.

use std::path::PathBuf;

use serde::Deserialize;

use super::enums::{ControlKind, PluginKind};

/// The parts of a manifest the *installer* cares about. A second view of
/// the same file as [`crate::layouts::PluginManifest`]; each ignores what
/// it does not know.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ManifestHeader {
    pub kind: PluginKind,
    pub extension: ExtensionManifest,
}

/// The `[extension]` section of a manifest: everything PolterType needs
/// to run a plug-in and show it, without the plug-in running any code to
/// describe itself — it must not execute before the user has seen what
/// it wants. Every field defaults, so an omitted section contributes
/// nothing.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ExtensionManifest {
    /// File name of the program, resolved inside the plug-in's own
    /// `bin/`. A plain name, never a path — see
    /// [`super::PluginError::BadExecutablePath`].
    pub exe: String,
    /// Argument that starts the long-running service, if it has one.
    /// Empty means the plug-in is only ever run for single commands.
    pub service_args: Vec<String>,
    /// Config file the settings pane edits, relative to the user's
    /// config directory. The plug-in owns this file; PolterType only
    /// writes the keys its pane declares.
    pub config_file: String,
    /// One-line description shown next to the plug-in in the UI.
    pub summary: String,
    /// Named commands the UI may invoke, as argument lists.
    pub commands: Vec<PluginCommand>,
    /// Entries to add to the tray menu.
    pub tray_items: Vec<TrayItem>,
    /// The settings pane, rendered natively by PolterType.
    pub pane: Vec<PaneControl>,

    /// Arguments to a command that prints the plug-in's current state,
    /// one `key=value` per line. Empty means it reports nothing and its
    /// tray entries are plain commands.
    ///
    /// Asked of the plug-in, never read from its config file: the config
    /// holds what it *starts* as, so a menu built from it would
    /// confidently show a value that runtime has since changed.
    pub state_args: Vec<String>,

    /// Menus built from rows the plug-in produces at runtime, rather
    /// than from entries the manifest could name in advance.
    pub tray_lists: Vec<TrayList>,

    /// Which key of the reported state means "this plug-in is waiting
    /// for you", counted rather than merely present. Empty (the default)
    /// never marks the icon; set, a value above zero raises a dot on
    /// PolterType's shared tray icon — a plug-in may raise that dot,
    /// never replace the icon, draw on it or choose how it looks.
    pub attention_state_key: String,
}

/// A tray submenu whose entries the plug-in supplies while the menu is
/// being opened — a queue, an inbox, anything [`TrayItem`] cannot name in
/// advance. The plug-in prints rows; PolterType draws them and runs only
/// the commands the manifest declared, so "produced at runtime" extends
/// to the *text*, never to what clicking it does.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TrayList {
    /// Title of the submenu. `{}` is replaced by the number of rows, so
    /// a count is visible without opening it.
    pub label: String,
    /// Shown, disabled, in place of the submenu when there are no rows.
    /// Empty means the whole thing is hidden while it is empty.
    pub empty_label: String,
    /// Id of the [`PluginCommand`] whose output is the rows, in the same
    /// tab-separated form the pane's tick-box lists use: `id`, then a
    /// label, then any number of detail fields.
    pub command: String,
    /// What may be done to one row. Each becomes an entry inside that
    /// row's own submenu, under its details.
    pub actions: Vec<TrayListAction>,
    /// What may be done to the whole list — emptying it, accepting all
    /// of it. Drawn once, below the rows, behind a separator.
    pub bulk: Vec<TrayListAction>,
}

/// One thing a runtime row can do: to a row of a [`TrayList`], to all of
/// them, or to one card of a repeating group in the settings pane.
///
/// One shape for all three so the substitution rule below exists once —
/// a row's identity reaching a command line is the step where a plug-in's
/// own output could turn into a flag.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TrayListAction {
    /// What the user reads.
    pub label: String,
    /// Id of the [`PluginCommand`] to run. For a per-row action, any
    /// argument equal to `{id}` is replaced by that row's id — the only
    /// substitution there is, and a whole argument rather than a
    /// fragment of one, so a row id can never become part of a flag.
    pub command: String,
}

/// A command the plug-in exposes, run as `<exe> <args…>`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PluginCommand {
    /// Referred to by tray items and buttons.
    pub id: String,
    /// What the user sees.
    pub label: String,
    pub args: Vec<String>,
}

/// A tray menu entry contributed by a plug-in.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TrayItem {
    /// What the user sees. When this entry reports state rather than
    /// acting (see [`Self::state_key`] with no [`Self::command`]),
    /// `{}` in the label is replaced by the current value.
    pub label: String,
    /// Id of a [`PluginCommand`]. Empty means this entry only *shows*
    /// state and cannot be clicked.
    pub command: String,

    /// Which key of the plug-in's reported state this entry reflects.
    ///
    /// Empty — a plain command with no state. Set **with**
    /// [`Self::state_value`] — the entry is ticked when the reported
    /// value matches, so a group reads as alternatives with the live one
    /// marked. Set **without** it — the entry is a disabled status line
    /// showing the value in its label, so menus that draw ticks faintly
    /// or not at all still say what is in force.
    pub state_key: String,

    /// The value of [`Self::state_key`] that ticks this entry.
    pub state_value: String,
}

impl TrayItem {
    /// Does this entry show a tick?
    pub fn is_check(&self) -> bool {
        !self.state_key.is_empty() && !self.state_value.is_empty()
    }

    /// Is this entry a read-only status line?
    pub fn is_status(&self) -> bool {
        !self.state_key.is_empty() && self.state_value.is_empty()
    }

    /// The label to render, given the plug-in's reported state.
    ///
    /// `state` is `None` when the plug-in could not be asked at all —
    /// rendered differently from "answered, but said nothing about this
    /// key", because the first is a plug-in to go and look at.
    pub fn render(&self, state: Option<&std::collections::HashMap<String, String>>) -> String {
        if self.is_check() {
            // The tick as a character, not only as a menu attribute:
            // tray backends draw a native checkmark faintly or not at
            // all. Padded so labels do not shift as the mark moves.
            return format!("{} {}", self.mark(state), self.label);
        }
        if !self.is_status() {
            return self.label.clone();
        }
        let value = match state {
            None => "not responding",
            Some(s) => s.get(&self.state_key).map_or("unknown", String::as_str),
        };
        if self.label.contains("{}") {
            self.label.replacen("{}", value, 1)
        } else {
            format!("{}: {value}", self.label)
        }
    }

    /// Is this alternative the live one?
    pub fn is_active(&self, state: Option<&std::collections::HashMap<String, String>>) -> bool {
        self.is_check()
            && state.is_some_and(|s| {
                s.get(&self.state_key)
                    .is_some_and(|v| *v == self.state_value)
            })
    }

    /// The glyph standing in for a tick. A space of the same width when
    /// inactive, so nothing jumps sideways when the mark moves.
    fn mark(&self, state: Option<&std::collections::HashMap<String, String>>) -> &'static str {
        if self.is_active(state) {
            "✓"
        } else {
            "\u{2007}"
        }
    }
}

/// One alternative offered by a [`ControlKind::Choice`].
///
/// Either a bare value — what every manifest wrote before an option could
/// explain itself — or a table that adds a sentence and a link. Both
/// forms live in the same `options` array, so a plug-in can describe the
/// three choices that need it and leave the other two as strings:
///
/// ```toml
/// options = [
///   "off",
///   { value = "qwen3:8b", label = "Qwen3 8B",
///     detail = "Fits an 8 GB card whole.",
///     link = "https://ollama.com/library/qwen3" },
/// ]
/// ```
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum PaneOption {
    /// Just the value; it is its own label and says nothing more.
    Value(String),
    Described {
        /// What is written into the plug-in's config when this is chosen.
        value: String,
        /// What to show instead of the raw value. Empty means the value.
        #[serde(default)]
        label: String,
        /// A sentence about this alternative, shown under it.
        #[serde(default)]
        detail: String,
        /// Where its makers describe it. `https` only — see
        /// `validate::check_pane`.
        #[serde(default)]
        link: String,
    },
}

impl PaneOption {
    /// The string written into the config.
    pub fn value(&self) -> &str {
        match self {
            Self::Value(v) => v,
            Self::Described { value, .. } => value,
        }
    }

    /// What the user reads. The value itself unless the plug-in named
    /// something friendlier.
    pub fn label(&self) -> &str {
        match self {
            Self::Value(v) => v,
            Self::Described { value, label, .. } => {
                if label.trim().is_empty() {
                    value
                } else {
                    label
                }
            }
        }
    }

    pub fn detail(&self) -> &str {
        match self {
            Self::Value(_) => "",
            Self::Described { detail, .. } => detail,
        }
    }

    pub fn link(&self) -> &str {
        match self {
            Self::Value(_) => "",
            Self::Described { link, .. } => link,
        }
    }

    /// Does this option carry anything beyond its value? A choice where
    /// none do is drawn as a drop-down, as it always was.
    pub fn is_described(&self) -> bool {
        !self.detail().trim().is_empty() || !self.link().trim().is_empty()
    }
}

/// One control in the plug-in's settings pane.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PaneControl {
    pub kind: ControlKind,
    /// Dotted key in the plug-in's config file (`act.mode`). Empty for
    /// [`ControlKind::Button`], which acts rather than stores.
    pub key: String,
    pub label: String,
    /// Longer explanation rendered under the control.
    pub help: String,
    /// Allowed values for [`ControlKind::Choice`].
    pub options: Vec<PaneOption>,
    /// Id of a [`PluginCommand`] — the one a [`ControlKind::Button`]
    /// runs, or the one whose output a [`ControlKind::Report`] shows.
    pub command: String,
    /// What one row of a [`ControlKind::Records`] holds: ordinary
    /// controls whose `key` is a bare field name relative to the row, not
    /// a dotted path. Nested records are refused — a pane that can nest
    /// is a config editor, and this is not one.
    pub fields: Vec<PaneControl>,
    /// Label for the button that appends a row. Empty gets "Add".
    pub add_label: String,

    /// What may be done to one row of a [`ControlKind::Records`] group
    /// beyond editing and removing it. Each becomes a small button on
    /// that row's card, running the declared command with `{id}`
    /// replaced by the row's [`Self::id_field`].
    pub actions: Vec<TrayListAction>,

    /// Which field of a row supplies the `{id}` its [`Self::actions`]
    /// are given. Required as soon as there is an action; a row has no
    /// identity of its own that a plug-in would recognise.
    pub id_field: String,
}

impl Default for PaneControl {
    fn default() -> Self {
        Self {
            kind: ControlKind::Toggle,
            key: String::new(),
            label: String::new(),
            help: String::new(),
            options: Vec::new(),
            command: String::new(),
            fields: Vec::new(),
            add_label: String::new(),
            actions: Vec::new(),
            id_field: String::new(),
        }
    }
}

/// The outcome of a successful install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPack {
    pub id: String,
    pub name: String,
    pub version: String,
    /// Where it now lives.
    pub path: PathBuf,
    /// Files copied in.
    pub files: usize,
    pub bytes: u64,
    /// Entries found in the source and deliberately not copied, relative
    /// to the source root. Surfaced rather than silently dropped, so a
    /// pack author learns a misplaced file was ignored.
    pub skipped: Vec<String>,
    /// Whether this replaced an existing pack of the same id.
    pub replaced: bool,
}
