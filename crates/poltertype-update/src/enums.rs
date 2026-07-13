//! Updater error type.

use std::io;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("could not locate a per-user data directory for poltertype")]
    NoDataDir,
    #[error("network: {0}")]
    Network(String),
    #[error("manifest is not valid JSON: {0}")]
    Manifest(#[from] serde_json::Error),
    /// The manifest declares a schema this build predates. Declining is
    /// the safe move: we would be guessing at the meaning of fields we
    /// have never seen, and the user can always install by hand.
    #[error("manifest schema {found} is newer than this build understands ({supported})")]
    UnsupportedSchema { found: u32, supported: u32 },
    #[error("manifest has no artifact for this platform ({0})")]
    NoArtifactForPlatform(String),
    #[error("manifest version `{0}` is not valid semver: {1}")]
    BadVersion(String, semver::Error),
    /// The bytes we got are not the bytes the release promised. Either
    /// the download corrupted or someone swapped the file — we cannot
    /// tell which, and we treat both the same way: delete and abort.
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("artifact is larger than the {0} byte ceiling")]
    TooLarge(u64),
    /// The running binary isn't in a shape this platform's installer
    /// knows how to replace — an AppImage started outside its
    /// AppImage wrapper, a `cargo run` dev build, a distro package.
    #[error("this install cannot update itself in place: {0}")]
    UnsupportedInstall(String),
    #[error("io: {0}")]
    Io(#[from] io::Error),
}

/// `ureq::Error` is big (it carries the whole response on a 4xx/5xx),
/// so we flatten it to its rendering at the boundary rather than
/// hauling it through the crate.
impl From<ureq::Error> for UpdateError {
    fn from(e: ureq::Error) -> Self {
        UpdateError::Network(e.to_string())
    }
}
