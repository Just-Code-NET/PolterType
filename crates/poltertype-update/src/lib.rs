//! Background updates from GitHub Releases.
//!
//! The **only** part of a default build that touches the network, on
//! one endpoint: the release manifest and the assets it points at.
//! Nothing about the user or their typing is sent — see [`check`] for
//! the exact request — and the whole subsystem is gated behind
//! `[updates].enabled`.
//!
//! 1. **Check** — `GET` [`consts::MANIFEST_URL`], GitHub's
//!    `releases/latest/download/<asset>` redirector, which resolves to
//!    the newest published non-prerelease release. Our workflow creates
//!    releases as drafts, so an unpublished build cannot reach a user.
//! 2. **Compare** — semver against the *running* binary ([`version`]).
//! 3. **Download** — into a staging directory, verifying SHA-256
//!    ([`download`]); a mismatch deletes the file and fails.
//! 4. **Stage** — record it in `pending.json` ([`staging`]). Nothing is
//!    installed yet.
//! 5. **Apply** — on Quit or an explicit "Restart to update", hand the
//!    artifact to the per-OS installer ([`apply`]) and exit. We never
//!    swap a binary out from under a running keyboard hook.
//!
//! The checksum comes from the same release as the artifact, so it
//! catches truncated downloads and a tampered asset CDN but **not** a
//! compromised GitHub account. That is what the manifest's detached
//! ed25519 [`signature`] is for, verified the moment the manifest is
//! parsed, before any URL in it is read.
//!
//! Signatures are **mandatory from v0.17.2** —
//! `consts::REQUIRE_SIGNATURE` gates that, and it is now `true`: an
//! unsigned manifest is refused rather than warned about. A release
//! whose manifest nobody signs is therefore an outage for every
//! updater on this build or newer. See `docs/DECISIONS.md`.

#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod apply;
mod check;
mod consts;
mod download;
mod enums;
mod manifest;
mod signature;
mod staging;
mod types;
mod version;

pub use apply::apply;
pub use check::{check_and_stage, current_version};
pub use consts::MANIFEST_URL;
pub use enums::UpdateError;
pub use manifest::platform_key;
pub use signature::signing_payload;
pub use staging::{clear_pending, read_pending};
pub use types::{Artifact, Manifest, PendingUpdate};
pub use version::is_newer;

#[cfg(test)]
mod tests;
