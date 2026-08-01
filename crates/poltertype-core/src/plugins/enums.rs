//! Why an install refused.

use std::path::PathBuf;

use thiserror::Error;

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
