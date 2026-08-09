//! What kind of plug-in this is, and why an install refused.

use std::path::PathBuf;

use serde::Deserialize;
use thiserror::Error;

/// The two kinds of plug-in, which are **not** two points on one scale:
/// they have different trust models and different validation rules.
///
/// A [`Self::Pack`] is data and cannot execute; the installer's
/// allow-list is what guarantees that.
///
/// A [`Self::Extension`] ships a program — a genuinely larger decision,
/// so it is a separate kind rather than a looser pack. The difference
/// is visible in the manifest, can be shown before anything is
/// installed, and cannot be arrived at by a pack quietly gaining a
/// field. An extension never runs *inside* PolterType; it is spawned as
/// its own process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginKind {
    /// Data only: layouts, wordlists, translations. The default, so a
    /// manifest written before this field existed still means what it
    /// meant.
    #[default]
    Pack,
    /// Ships an executable that PolterType supervises as a separate
    /// process and surfaces in its UI.
    Extension,
}

impl PluginKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pack => "pack",
            Self::Extension => "extension",
        }
    }
}

/// What a control in a plug-in's settings pane looks like.
///
/// Deliberately a small, closed set. PolterType renders these itself,
/// natively, so a plug-in describes *what* it wants configured and
/// never gets to draw anything — which is what keeps a third-party
/// pane from imitating a system prompt or PolterType's own UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ControlKind {
    /// On/off, bound to a boolean key.
    Toggle,
    /// One of a fixed list of `options`.
    Choice,
    /// Free text, one line.
    Text,
    /// A whole number, bound to an integer key.
    Number,
    /// Runs one of the plug-in's declared commands. Binds to no key.
    Button,
    /// Shows what one of the plug-in's declared commands prints.
    ///
    /// The only control that displays something the plug-in produced
    /// rather than something the manifest declared, so: the text is
    /// rendered as text, in a fixed-width block visibly part of the
    /// plug-in's card. It cannot draw a button, style itself, or look
    /// like anything PolterType said. It exists for the answer a
    /// plug-in has and a manifest cannot — what it found, what it can
    /// and cannot do on this machine.
    ///
    /// Binds to no key. Reads, never writes.
    Report,
    /// A checkbox per row, where the rows come from the plug-in and
    /// ticking one adds its name to an array in the plug-in's config.
    ///
    /// The control for a set nobody can write down in advance — which
    /// applications to learn from, which to act in. A manifest cannot
    /// list what is installed on this machine, so the plug-in supplies
    /// the rows at runtime and PolterType draws the boxes. A row may
    /// carry a line of detail, which is how it says what it *measured*
    /// rather than only naming the application.
    ///
    /// `key` is the array. `command` produces the rows.
    List,
    /// A control this PolterType does not know.
    ///
    /// Here so a plug-in written for a newer PolterType still *loads* on
    /// an older one. Without it serde refuses the unknown word, the
    /// refusal takes the whole manifest with it, and the plug-in
    /// vanishes from the pane because one line mentioned a control that
    /// did not exist yet. One unrenderable control is smaller and
    /// explains itself — the pane says it needs a newer version.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Error)]
pub enum PluginError {
    #[error("{0} does not exist or is not a directory")]
    NotADirectory(PathBuf),
    #[error("no {manifest} in {dir} — every pack needs one")]
    MissingManifest { dir: PathBuf, manifest: String },
    #[error("manifest is not valid TOML: {0}")]
    BadManifest(String),
    #[error("manifest has no `id`, or it is empty")]
    MissingId,
    /// Pack ids become a directory name, so they are restricted to
    /// characters that cannot escape one or surprise a shell.
    #[error(
        "pack id {0:?} is not usable as a directory name — use letters, digits, `-`, `_` and `.`"
    )]
    UnsafeId(String),
    #[error("pack is {actual} bytes, over the {limit}-byte limit")]
    TooLarge { actual: u64, limit: u64 },
    #[error("pack has more than {0} files")]
    TooManyFiles(usize),
    /// A path that would land outside the pack directory, or a
    /// symlink. Refused rather than resolved.
    #[error("refusing unsafe path in pack: {0}")]
    UnsafePath(PathBuf),
    #[error("pack contains nothing installable — no layouts, wordlists or translations")]
    Empty,
    /// An extension declared no program to run, or named one that is
    /// not in the pack. Refused rather than installed as a pack: a
    /// half-declared extension would silently become inert.
    #[error("extension declares no runnable program: {0}")]
    NoExecutable(String),
    /// The declared executable is not where it must be. Extensions may
    /// only run something from their own `bin/` directory, so a
    /// manifest cannot point PolterType at an arbitrary path on the
    /// user's disk.
    #[error("extension executable {0:?} must be a plain file name inside bin/")]
    BadExecutablePath(String),
    /// A pane control that cannot be rendered — an unknown key, a
    /// choice with no options, a control bound to nothing.
    #[error("settings pane is not usable: {0}")]
    BadPane(String),
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },
}

impl PluginError {
    pub(crate) fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }
}
