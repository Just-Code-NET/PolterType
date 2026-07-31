//! Background updates from GitHub Releases.
//!
//! This crate is the **only** part of a default PolterType build that
//! touches the network, and it does so on one endpoint: the release
//! manifest and the release assets it points at. Nothing about the
//! user or their typing is sent — see [`check`] for the exact request.
//! The whole subsystem is gated behind `[updates].enabled` in
//! `config.toml`.
//!
//! ## The flow
//!
//! 1. **Check** — `GET` the manifest at [`consts::MANIFEST_URL`]. That
//!    is GitHub's `releases/latest/download/<asset>` redirector, which
//!    resolves to the newest **published, non-prerelease** release. Our
//!    release workflow creates releases as *drafts*, so an unpublished
//!    build can never reach a user.
//! 2. **Compare** — parse `version` as semver and compare against
//!    `CARGO_PKG_VERSION` of the *running* binary (see [`version`]).
//! 3. **Download** — fetch the artifact for this platform into a
//!    staging directory and verify its SHA-256 against the manifest
//!    ([`download`]). A mismatch deletes the file and fails the check.
//! 4. **Stage** — record the verified download in `pending.json`
//!    ([`staging`]). Nothing is installed yet.
//! 5. **Apply** — on Quit, or when the user clicks the tray's "Restart
//!    to update", hand the staged artifact to the per-OS installer
//!    ([`apply`]) and exit. We never swap a binary out from under a
//!    running keyboard hook.
//!
//! ## What the checksum does and does not buy us
//!
//! The SHA-256 comes from the same release as the artifact, so it
//! catches truncated downloads and a tampered *asset* CDN, but it does
//! **not** defend against a compromised GitHub account: whoever can
//! publish a release can publish a matching checksum. That is what the
//! manifest's `signature` field is for — a detached ed25519 signature
//! made on the maintainer's machine, with a key CI never sees, checked
//! against a public key compiled into this binary ([`signature`]).
//! It is verified the moment the manifest is parsed, before any URL in
//! it is read.
//!
//! Signatures are **not yet mandatory**: `consts::REQUIRE_SIGNATURE`
//! gates that, and flipping it is a deliberate second step once signed
//! releases are the ones users' updaters resolve to. Until then a
//! present signature must verify and a missing one only warns — so do
//! not describe the updater as "signed" anywhere user-facing until the
//! flip has happened. See `docs/DECISIONS.md`.

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
