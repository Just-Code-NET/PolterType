//! What an install reports back.

use std::path::PathBuf;

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
