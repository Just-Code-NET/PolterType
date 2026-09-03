//! Per-OS dispatch for the setup probe and its actions.

#[cfg(target_os = "linux")]
use super::linux as imp;
#[cfg(target_os = "macos")]
use super::macos as imp;
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
use super::unsupported as imp;
#[cfg(windows)]
use super::windows as imp;

use super::enums::Permission;
use super::types::SetupReport;

/// Probe the current machine. Cheap enough to call on every *Check
/// again* click — a handful of `stat`s and one framework call — and
/// deliberately not cached, since the entire point is to notice that
/// the user just flipped a switch.
/// `local_signing_identity` is `[updates].local_signing_identity` — the
/// macOS pane adds a step about keeping permissions across updates and
/// needs to know whether the machinery is already configured. The
/// other platforms ignore it.
pub fn probe_setup(local_signing_identity: &str) -> SetupReport {
    imp::probe(local_signing_identity)
}

/// Trigger the OS's own permission dialog (macOS only).
///
/// Returns whether the permission is granted after the call. On
/// Accessibility that answer is usually `false` even when all is well:
/// the dialog is asynchronous, so the honest reading is "the user has
/// been asked", and the pane re-probes rather than believing this.
/// Everywhere else there is no such dialog and this is a no-op.
pub fn request_permission(permission: Permission) -> bool {
    imp::request(permission)
}

/// Where to send a user whose system dialog will never appear again —
/// macOS shows each prompt once, and after that the only route is the
/// Settings pane itself.
pub fn permission_settings_url(permission: Permission) -> Option<&'static str> {
    imp::settings_pane_url(permission)
}

/// Create (or adopt) the local code-signing identity the updater
/// re-signs swapped bundles with, so the TCC grants survive updates.
///
/// macOS only. Idempotent: an identity of that name already in the
/// keychain is adopted rather than duplicated. The caller writes the
/// name into `[updates].local_signing_identity` on `Ok`.
pub fn setup_local_signing(name: &str) -> Result<(), String> {
    imp::setup_local_signing(name)
}
