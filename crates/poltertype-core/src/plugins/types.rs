//! What an install reports back, and what an extension declares.

use std::path::PathBuf;

use serde::Deserialize;

use super::enums::{ControlKind, PluginKind};

/// The parts of a manifest the *installer* cares about, read
/// separately from [`crate::layouts::PluginManifest`] so that adding
/// extensions did not have to reach into the layout loader's types.
///
/// Both views parse the same file; each ignores what it does not know.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ManifestHeader {
    pub kind: PluginKind,
    pub extension: ExtensionManifest,
}

/// The `[extension]` section of a manifest: everything PolterType needs
/// to run a plug-in and show it, without the plug-in running any code
/// to describe itself.
///
/// That last part is the point. A plug-in that had to be *started* in
/// order to say what it contributes would have to run before the user
/// had seen what it wants — so all of this is static, readable from
/// disk, and shown before anything is launched.
///
/// Every field defaults, so a manifest that omits a section simply
/// contributes nothing there.
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
    /// one `key=value` per line. Empty means the plug-in reports
    /// nothing and its tray entries are plain commands.
    ///
    /// Asking the plug-in, rather than reading its config file, is the
    /// only answer that can be right: the config holds what a plug-in
    /// *starts* as, and anything a user changes at runtime — a mode
    /// armed from the command line, an authority that has since expired
    /// — lives elsewhere or nowhere. A menu showing the config value
    /// would confidently display the wrong thing, which is worse than
    /// the blank menu it replaced.
    pub state_args: Vec<String>,
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
    /// Empty — the entry is a plain command with no state, and renders
    /// exactly as before.
    ///
    /// Set, **with** [`Self::state_value`] — the entry gets a tick when
    /// the reported value matches, so a group of them reads as a set of
    /// alternatives with the live one marked.
    ///
    /// Set, **without** [`Self::state_value`] — the entry is a disabled
    /// status line showing the value in its label. Menus that draw
    /// ticks faintly, or not at all, still say what is in force.
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
    /// `state` is `None` when the plug-in could not be asked at all.
    /// That is worth saying differently from "answered, but said
    /// nothing about this key": the first is a plug-in to go and look
    /// at, the second is ordinary. A blank would be neither, and would
    /// leave the user guessing which.
    pub fn render(&self, state: Option<&std::collections::HashMap<String, String>>) -> String {
        if self.is_check() {
            // The tick as a character, not only as a menu attribute.
            // Tray backends draw a native checkmark differently, faintly
            // or not at all, and an indicator you cannot see is the bug
            // this whole mechanism exists to fix. Padded so the labels
            // do not shift as the mark moves between them.
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
    pub options: Vec<String>,
    /// Id of a [`PluginCommand`], for [`ControlKind::Button`].
    pub command: String,
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
    /// Entries found in the source and deliberately not copied,
    /// relative to the source root.
    ///
    /// Surfaced rather than silently dropped: a pack author who put a
    /// file somewhere unexpected should learn that it was ignored,
    /// and a user installing someone else's pack should see that it
    /// tried to ship something a language pack has no business
    /// shipping.
    pub skipped: Vec<String>,
    /// Whether this replaced an existing pack of the same id.
    pub replaced: bool,
}
