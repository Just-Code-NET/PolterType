//! Endpoints, limits and on-disk names for the updater.

/// The release manifest. `releases/latest/download/<asset>` is GitHub's
/// redirector to the newest **published, non-prerelease** release — the
/// gate between "CI built it" and "users get it".
///
/// Not configurable at runtime: a knob pointing the updater at an
/// arbitrary host would turn a hand-edited `config.toml` into a
/// code-execution vector. Public so the Settings window can show the
/// exact URL the app talks to.
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

/// Left in [`STAGING_DIR`] by an installer the OS refused, carrying the
/// exit code. Read back on the next start — an update that fails must
/// say so, not look like a restart that did nothing.
pub(crate) const FAILED_FILE: &str = "install-failed.txt";

/// The app's rolling-log directory, relative to the data directory.
/// Matches `SettingsStore::log_dir`, which resolves the same
/// `ProjectDirs` triple: the installer's own log belongs where the
/// tray's "Open Logs Folder…" already points.
pub(crate) const LOG_DIR: &str = "logs";

/// Everything the installer script prints. Outside [`STAGING_DIR`] on
/// purpose: the successful path deletes that directory, and a log file
/// still open by the installer is what would make the deletion fail.
pub(crate) const INSTALLER_LOG: &str = "installer.log";

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

/// Whether a manifest without a signature is refused. `true` since
/// v0.17.2; see docs/DECISIONS.md, 2026-08-16, for the two-stage
/// rollout that got here.
///
/// **The trap:** an unsigned release is no longer a warning, it is
/// every updater on v0.17.2+ refusing to see it, until somebody signs
/// and re-uploads `latest.json`. Signing is manual by design — the
/// private key must not be a CI secret, since the attacker it defends
/// against is someone who can publish a release — so the only thing
/// standing between us and that outage is `docs/RELEASING.md` §7.
///
/// Setting it back to `false` is the recovery path if the key is ever
/// lost; older builds carry their own copy and are unaffected.
pub(crate) const REQUIRE_SIGNATURE: bool = true;
