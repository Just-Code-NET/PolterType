//! What kind of plug-in this is, and why an install refused.

use std::path::PathBuf;

use serde::Deserialize;
use thiserror::Error;

/// The two kinds of plug-in, which are **not** two points on one
/// scale — they have different trust models and are validated by
/// different rules.
///
/// A [`Self::Pack`] is data. It cannot execute; the installer's
/// allow-list is what guarantees that, and nothing about it changes.
///
/// A [`Self::Extension`] ships a program. That is a genuinely larger
/// decision by the user, so it is a separate kind rather than a looser
/// pack: the difference is visible in the manifest, can be shown in
/// the UI before anything is installed, and cannot be arrived at by a
/// pack quietly gaining a field.
///
/// An extension never runs *inside* PolterType. It is spawned as its
/// own process, so it has its own permissions and its own crashes, and
/// the process that owns the global keyboard hook is not put at the
/// mercy of third-party code.
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
    /// rather than something the manifest declared — so it is worth
    /// being precise about what that does and does not allow. The text
    /// is rendered as text, in a fixed-width block that is visibly part
    /// of the plug-in's card: it cannot draw a button, style itself,
    /// or look like anything PolterType said. What it is for is the
    /// answer a plug-in has and a manifest cannot: how much it has
    /// learned, what it found, what it will and will not be able to do
    /// on this machine.
    ///
    /// Binds to no key. Reads, never writes.
    Report,
    /// A control this PolterType does not know.
    ///
    /// Here so that a plug-in written for a newer PolterType still
    /// *loads* on an older one. Without it, serde refuses the unknown
    /// word, the refusal takes the whole manifest with it, and the
    /// user's plug-in vanishes from the pane entirely because one line
    /// mentioned a control that had not been invented when their app
    /// was built. One unrenderable control is a far smaller problem,
    /// and it is a problem that explains itself: the pane says the
    /// control needs a newer version.
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
