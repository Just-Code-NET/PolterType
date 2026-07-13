//! The `latest.json` schema and the staged-update bookkeeping.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The release manifest, published as an asset named `latest.json` on
/// every GitHub Release. Written by `.github/workflows/release.yml`;
/// keep the two in step.
///
/// ```json
/// {
///   "schema": 1,
///   "version": "0.4.0",
///   "notes_url": "https://github.com/Just-Code-NET/PolterType/releases/tag/v0.4.0",
///   "artifacts": {
///     "linux-x86_64": {
///       "url": "https://github.com/.../poltertype-0.4.0-x86_64.AppImage",
///       "sha256": "9f86d0…",
///       "size": 28311552
///     }
///   }
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    /// Bumped only for a breaking change to this struct. See
    /// `consts::SUPPORTED_SCHEMA`.
    pub schema: u32,
    /// Release version, without the `v` prefix the git tag carries.
    pub version: String,
    /// Human-readable release notes — what the tray's "What's new"
    /// link opens. Never fetched by the app, only handed to the
    /// browser.
    pub notes_url: String,
    /// Keyed by [`crate::platform_key`]: `windows-x86_64`,
    /// `macos-universal`, `linux-x86_64`. A platform missing from the
    /// map simply gets no update — that is how we ship a release that
    /// deliberately skips an OS.
    pub artifacts: HashMap<String, Artifact>,
    /// Reserved for a detached ed25519 signature over the manifest
    /// bytes. Absent today: the current trust model is HTTPS plus the
    /// per-artifact checksum, which does not survive a compromised
    /// GitHub account. Declared now so adding real signing later is a
    /// value change, not a schema break.
    #[serde(default)]
    pub signature: Option<String>,
}

/// One installable file: where to get it and what it must hash to.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Artifact {
    pub url: String,
    /// Lowercase hex SHA-256 of the file at `url`.
    pub sha256: String,
    /// Bytes. Used to reject a response whose length disagrees with
    /// the manifest before we spend the download, and to show a size
    /// in the UI.
    pub size: u64,
}

/// A verified artifact sitting in the staging directory, waiting for a
/// moment when installing it won't yank the keyboard hook out from
/// under the user. Serialised to `pending.json` so it survives a
/// restart — a download interrupted by a reboot shouldn't have to
/// happen twice.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingUpdate {
    /// Version of the *staged* artifact, not of the running binary.
    pub version: String,
    pub notes_url: String,
    /// Absolute path of the downloaded, checksum-verified file.
    pub artifact: PathBuf,
    /// How many times we have handed this file to the OS installer.
    /// Incremented before each attempt, so a file that reliably kills
    /// the installer still gets discarded after
    /// `consts::MAX_INSTALL_ATTEMPTS` rather than retried forever.
    #[serde(default)]
    pub attempts: u32,
}
