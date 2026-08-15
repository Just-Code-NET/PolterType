//! Endpoints, limits and on-disk names for the updater.

/// The release manifest. `releases/latest/download/<asset>` is GitHub's
/// own redirector to the newest **published, non-prerelease** release,
/// which is exactly the gate we want between "CI built it" and "users
/// get it".
///
/// Not configurable at runtime, on purpose: a knob pointing the updater
/// at an arbitrary host would turn a hand-edited `config.toml` into a
/// code-execution vector. Public so the Settings window can show the
/// exact URL the app talks to — "it phones home" should be checkable
/// rather than taken on faith.
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
/// Signing and verifying landed together in v0.7.0 but could not
/// become mandatory in the same release: a user on that build would
/// have been checking a manifest published before anyone signed one.
/// So the rollout was two stages, and **v0.17.2 is the second**.
///
/// 1. **`false`, v0.7.0 → v0.17.1** — a signature that is *present*
///    must verify and a wrong one is refused loudly; a missing one
///    only warns. This is when signed manifests start being published.
/// 2. **`true`, from v0.17.2** — every release from v0.7.0 to v0.17.1
///    was in fact signed, checked live through the redirector, so the
///    manifest a user's updater resolves to has carried a signature
///    for eighteen releases. From here an attacker who can publish a
///    GitHub release can no longer publish an update.
///
/// **What this now costs us:** a release whose manifest nobody signs
/// is not a warning any more, it is every updater on v0.17.2+ refusing
/// to see it — an outage that lasts until somebody signs and re-uploads
/// `latest.json`. Signing stays a manual step by design (the private
/// key must not be a CI secret, since the attacker it defends against
/// is someone who can publish a release), so the thing standing
/// between us and that outage is `docs/RELEASING.md` §7. Nothing else
/// checks.
///
/// Older builds are unaffected: they carry their own copy of this
/// constant, still `false`, and go on accepting what they always did.
pub(crate) const REQUIRE_SIGNATURE: bool = true;
