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
    pub label: String,
    /// Id of a [`PluginCommand`].
    pub command: String,
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
