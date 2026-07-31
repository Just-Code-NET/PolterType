//! Fetching `latest.json` and picking this machine's artifact out of it.

use std::io::Read;
use std::time::Duration;

use tracing::debug;

use crate::consts::*;
use crate::enums::UpdateError;
use crate::types::{Artifact, Manifest};

/// Which artifact in the manifest belongs to the running build.
///
/// These strings are the manifest's map keys — they must match what
/// `.github/workflows/release.yml` writes. Deliberately coarse: we ship
/// one AppImage (x86_64), one universal DMG, one MSI (x86_64), so the
/// key space is exactly as wide as the release matrix and no wider.
/// An architecture we don't publish for resolves to a key that simply
/// isn't in the map, and the check ends with "no update for you"
/// rather than a wrong download.
pub fn platform_key() -> String {
    let os = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };

    // The DMG is a universal binary, so macOS gets one key regardless
    // of whether we're on Apple silicon or Intel.
    if os == "macos" {
        return "macos-universal".to_owned();
    }

    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "unknown"
    };
    format!("{os}-{arch}")
}

/// `GET` the release manifest.
///
/// The whole privacy surface of the updater is this request: a plain
/// HTTPS GET of a static asset, carrying no query string, no cookies
/// and no body. GitHub necessarily sees the connecting IP and the
/// User-Agent (which names our version) — that is unavoidable for any
/// update check, and it is what `[updates].enabled = false` switches
/// off.
pub(crate) fn fetch() -> Result<Manifest, UpdateError> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(MANIFEST_TIMEOUT_SECS))
        .user_agent(USER_AGENT)
        .build();

    let resp = agent.get(MANIFEST_URL).call()?;

    // Cap the read rather than trusting Content-Length: a hostile or
    // broken server can lie about the header but it cannot make us read
    // past `take`.
    let mut body = String::new();
    resp.into_reader()
        .take(MAX_MANIFEST_BYTES)
        .read_to_string(&mut body)?;

    let manifest: Manifest = serde_json::from_str(&body)?;
    if manifest.schema > SUPPORTED_SCHEMA {
        return Err(UpdateError::UnsupportedSchema {
            found: manifest.schema,
            supported: SUPPORTED_SCHEMA,
        });
    }
    // Before anything reads a URL out of this: a manifest we cannot
    // authenticate decides nothing here.
    crate::signature::verify(&manifest)?;
    debug!(
        version = %manifest.version,
        schema = manifest.schema,
        platforms = manifest.artifacts.len(),
        "fetched release manifest"
    );
    Ok(manifest)
}

/// The artifact for this platform, or a clear error naming the key we
/// looked for — the message ends up in the log when a release forgets
/// to publish for an OS.
pub(crate) fn pick<'m>(manifest: &'m Manifest, key: &str) -> Result<&'m Artifact, UpdateError> {
    manifest
        .artifacts
        .get(key)
        .ok_or_else(|| UpdateError::NoArtifactForPlatform(key.to_owned()))
}
