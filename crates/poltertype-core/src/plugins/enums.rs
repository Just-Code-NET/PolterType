//! What kind of plug-in this is, and why an install refused.

use std::path::PathBuf;

use serde::Deserialize;
use thiserror::Error;

/// The two kinds of plug-in, which are **not** two points on one scale:
/// they have different trust models and different validation rules.
///
/// A [`Self::Pack`] is data and cannot execute; the installer's
/// allow-list is what guarantees that. A [`Self::Extension`] ships a
/// program, spawned as its own process and never run *inside*
/// PolterType. A separate kind rather than a looser pack, so the
/// difference is declared in the manifest and visible before install
/// rather than reached by a pack quietly gaining a field.
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
    /// Free text with likely answers offered beside it, in a box that
    /// narrows the list as it is typed into — for a value that has
    /// *candidates* but no closed set, where a [`Self::Choice`] could not
    /// express an answer nothing enumerated.
    ///
    /// The candidates come from `options`, from `command` — the same
    /// tab-separated rows a [`Self::List`] ticks — or from both. The
    /// **id** is what gets written, never the label.
    Suggest,
    /// A whole number, bound to an integer key.
    Number,
    /// A fractional number, bound to a floating-point key.
    ///
    /// Separate from [`Self::Number`] because TOML's two number types are
    /// not interchangeable to the program reading the file: a plug-in
    /// expecting `0.35` and handed `1` fails to parse its own config.
    /// This control always writes a float, even for a round number.
    Decimal,
    /// A list of strings, edited as one comma-separated line. Prefer
    /// [`Self::List`] wherever the plug-in *can* enumerate the
    /// candidates, because a checkbox cannot be misspelt.
    Strings,
    /// A heading that groups the controls under it into a page of its
    /// own. Binds to no key and stores nothing; PolterType draws the
    /// sections as a navigation list and shows one at a time. A control
    /// belongs to the nearest section above it.
    Section,
    /// Runs one of the plug-in's declared commands. Binds to no key.
    Button,
    /// Shows what one of the plug-in's declared commands prints. Binds to
    /// no key; reads, never writes.
    ///
    /// The only control displaying something the plug-in produced rather
    /// than something the manifest declared, so it renders as plain text
    /// in a fixed-width block: it cannot draw a button, style itself, or
    /// look like anything PolterType said.
    Report,
    /// A checkbox per row, where the rows come from the plug-in and
    /// ticking one adds its id to an array in the plug-in's config — for
    /// a set a manifest cannot write down in advance, such as what is
    /// installed on this machine. A row may carry a line of detail.
    ///
    /// `key` is the array. `command` produces the rows.
    List,
    /// A repeating group: an array of tables in the plug-in's config,
    /// drawn as one card per entry with the plug-in's declared fields
    /// inside, plus Add and Remove.
    ///
    /// `key` is the array of tables (`schedule.sends`); `fields` declares
    /// what one row holds, as ordinary controls with keys relative to the
    /// row.
    Records,
    /// A control this PolterType does not know.
    ///
    /// Here so a plug-in written for a newer PolterType still *loads* on
    /// an older one: without it serde refuses the unknown word and the
    /// refusal takes the whole manifest — and the plug-in — with it.
    #[serde(other)]
    Unknown,
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
