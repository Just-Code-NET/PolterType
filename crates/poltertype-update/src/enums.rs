//! Updater error type.

use std::io;

use thiserror::Error;

/// What [`crate::apply`] managed to arrange, and therefore whether the
/// caller may now quit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applied {
    /// The update is on its way in and the caller must exit — either
    /// an installer is waiting for this process to disappear, or the
    /// new build is already in place and only the restart is left.
    HandedOff,
    /// Installed, but nothing on this session can start us again, so
    /// the caller must **keep running**: the next launch picks the new
    /// build up, and quitting here would only take the app away.
    InstalledStayUp,
    /// Refused too many times; the artifact has been deleted.
    Discarded,
}

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("could not locate a per-user data directory for poltertype")]
    NoDataDir,
    #[error("network: {0}")]
    Network(String),
    #[error("manifest is not valid JSON: {0}")]
    Manifest(#[from] serde_json::Error),
    /// The manifest declares a schema this build predates. Declining
    /// beats guessing at fields we have never seen; the user can always
    /// install by hand.
    #[error("manifest schema {found} is newer than this build understands ({supported})")]
    UnsupportedSchema { found: u32, supported: u32 },
    #[error("manifest has no artifact for this platform ({0})")]
    NoArtifactForPlatform(String),
    /// Signed, but not by us — or not over these bytes.
    /// Indistinguishable from tampering, and treated as such: the check
    /// stops before any URL in the manifest is read.
    #[error("manifest signature does not verify: {0}")]
    BadSignature(String),
    /// No `signature` field, in a build that requires one. See
    /// `consts::REQUIRE_SIGNATURE`.
    #[error("release manifest is unsigned and this build requires a signature")]
    UnsignedManifest,
    /// A field contains a newline, so two different manifests could
    /// render to the same signed payload. See `signature.rs`.
    #[error("manifest field `{0}` contains a line break and cannot be signed or verified")]
    UnsignablePayload(String),
    /// The compiled-in public key is not a usable ed25519 key — only
    /// reachable if `release-signing-key.pub` was corrupted in the
    /// build, never by anything a server said.
    #[error("the signing key built into this binary is unusable: {0}")]
    TrustedKeyBroken(String),
    #[error("manifest version `{0}` is not valid semver: {1}")]
    BadVersion(String, semver::Error),
    /// Corrupted download or a swapped file — indistinguishable, and
    /// both end the same way: delete and abort.
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("artifact is larger than the {0} byte ceiling")]
    TooLarge(u64),
    /// The running binary isn't in a shape this platform's installer
    /// knows how to replace — an AppImage started outside its
    /// AppImage wrapper, a `cargo run` dev build, a distro package.
    #[error("this install cannot update itself in place: {0}")]
    UnsupportedInstall(String),
    /// The OS created the installer process and it died without
    /// executing anything. Distinguishable only because every script
    /// announces itself first — see `apply::HELLO`.
    #[error("the installer process started and stopped without running: {0}")]
    InstallerSilent(String),
    /// The new build is installed and the app cannot bring itself
    /// back. Never a failed install, and must not be reported as one.
    #[error("could not arrange a restart: {0}")]
    RelaunchFailed(String),
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
