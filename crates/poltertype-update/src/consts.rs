//! Endpoints, limits and on-disk names for the updater.

/// The release manifest. `releases/latest/download/<asset>` is
/// GitHub's own redirector to the newest **published, non-prerelease**
/// release — it skips drafts and pre-releases for us, which is exactly
/// the gate we want between "CI built it" and "users get it".
///
/// Not configurable at runtime, on purpose: a settings knob pointing
/// the updater at an arbitrary host would turn a hand-edited
/// `config.toml` into a code-execution vector.
/// Public so the Settings window can show the user the exact URL the
/// app talks to. "It phones home" is a claim the user should be able
/// to check, not take on faith.
pub const MANIFEST_URL: &str =
    "https://github.com/Just-Code-NET/PolterType/releases/latest/download/latest.json";

/// Sent on every request so the traffic is attributable in GitHub's
/// logs and we can be blocked cleanly if we ever misbehave.
pub(crate) const USER_AGENT: &str = concat!("PolterType/", env!("CARGO_PKG_VERSION"), " (updater)");

/// Manifest fetch: a few KB of JSON. If it can't be had in 15 s the
/// network is not in a state where we want to start a download either.
pub(crate) const MANIFEST_TIMEOUT_SECS: u64 = 15;

/// Artifact download. Generous: the installers are 20–40 MB and users
/// on slow links are exactly the ones we shouldn't strand on an old
/// version. The worker thread is detached, so a long download blocks
/// nothing.
pub(crate) const DOWNLOAD_TIMEOUT_SECS: u64 = 600;

/// Hard ceiling on the artifact size, enforced while streaming. Guards
/// against a redirect to something enormous filling the user's disk;
/// our biggest installer is well under a tenth of this.
pub(crate) const MAX_ARTIFACT_BYTES: u64 = 300 * 1024 * 1024;

/// Manifest sanity ceiling — it is a handful of KB of JSON.
pub(crate) const MAX_MANIFEST_BYTES: u64 = 256 * 1024;

/// Subdirectory of the app's data dir where verified artifacts wait to
/// be installed.
pub(crate) const STAGING_DIR: &str = "updates";

/// Bookkeeping for the artifact staged in [`STAGING_DIR`].
pub(crate) const PENDING_FILE: &str = "pending.json";

/// Give up on a staged update after this many failed install attempts
/// and delete it. Without this, an artifact that the OS installer
/// rejects every single time would be retried on every quit, forever.
pub(crate) const MAX_INSTALL_ATTEMPTS: u32 = 3;

/// Manifest schema we know how to read. A newer app can widen this;
/// an *older* app seeing a bumped number declines the update rather
/// than guessing at fields it has never heard of.
pub(crate) const SUPPORTED_SCHEMA: u32 = 1;

/// Ed25519 public key the release manifest is checked against, base64
/// of the raw 32 bytes.
///
/// Compiled in, not fetched: a key the updater downloads is a key an
/// attacker can replace. Rotating it therefore means shipping a
/// release — which is the point, since the release binary is the thing
/// the user already decided to trust. The private half lives on the
/// maintainer's machine and never enters CI; see `docs/RELEASING.md`.
pub(crate) const TRUSTED_PUBLIC_KEY: &str = include_str!("../release-signing-key.pub");

/// First line of the signed payload. Domain-separates our signatures:
/// a signature made over some other document with this key can never
/// be replayed as a manifest signature.
pub(crate) const PAYLOAD_HEADER: &str = "poltertype-manifest-v1";

/// Whether a manifest without a signature is refused.
///
/// **The staged rollout, in two releases.** Signing and verifying land
/// together, but they cannot become mandatory in the same release: a
/// user running 0.7.0 would be checking a manifest published before
/// anyone signed one. So:
///
/// 1. **`false` (now).** A signature that is *present* must verify — a
///    wrong one is refused, loudly. A missing one is accepted with a
///    warning. This is the release in which signed manifests start
///    being published.
/// 2. **`true` (a later release).** Flip this the moment the manifest
///    that `releases/latest/download/latest.json` resolves to is
///    signed, and has been for a full release cycle. From then on an
///    unsigned manifest is an error, and an attacker who can publish a
///    GitHub release can no longer publish an update.
///
/// Flipping it early strands every user on a "cannot update" error
/// until the next release is published *and* signed. Flipping it never
/// means the signature is decorative.
pub(crate) const REQUIRE_SIGNATURE: bool = false;
