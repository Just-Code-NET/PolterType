//! Make the OS start PolterType at login, or stop it.
//!
//! `[general].autostart` shipped as a setting long before anything
//! honoured it: the Settings checkbox wrote `config.toml` and no code
//! ever read the value, on any platform. This crate is where the
//! checkbox becomes real.
//!
//! ## Platform coverage
//!
//! | Platform | Mechanism | Entry |
//! |---|---|---|
//! | macOS | per-user LaunchAgent + `launchctl` | `~/Library/LaunchAgents/<id>.plist` |
//! | Windows | `HKCU` run key via `reg.exe` | `…\CurrentVersion\Run`, value `<id>` |
//! | Linux | XDG autostart entry | `$XDG_CONFIG_HOME/autostart/<id>.desktop` |
//! | other | noop | — |
//!
//! This crate is one of the platform-code islands (see
//! `CONTRIBUTING.md`): `#[cfg(target_os)]` is allowed here, and
//! `poltertype-app` holds none. It is deliberately *not* a trait +
//! factory like `poltertype-layout` — there is no backend for the
//! caller to hold, just one idempotent operation, so the seam is a
//! single function in the shape `poltertype-tray` uses.
//!
//! ## Why no registry binding and no `unsafe`
//!
//! Each backend talks to the mechanism the OS already ships: two
//! `launchctl` calls, one `reg.exe` call, or a file write. That buys
//! a crate with no per-OS dependency and `#![forbid(unsafe_code)]`,
//! at the cost of spawning a short-lived process on two platforms —
//! which happens at most twice per app start. A typed registry
//! binding would be faster and add a dependency, a feature-gated
//! `windows` build and an `unsafe` block to save microseconds in code
//! that runs once. (On Windows the child is spawned with
//! `CREATE_NO_WINDOW`, or a console would flash at every login.)
//!
//! ## What this crate does not do
//!
//! It never *reads* the OS to discover intent — `config.toml` is the
//! single source of truth, and [`sync`] only pushes that truth
//! outwards. If a user deletes the LaunchAgent by hand, the next
//! launch puts it back. That is the intended direction: the setting
//! owns the entry, not the other way round.

#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod types;

#[cfg(test)]
mod tests;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as imp;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as imp;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as imp;

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod noop;
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
use noop as imp;

pub use types::App;

/// Make the OS autostart entry match `enabled`.
///
/// Idempotent and safe to call often — at startup and after every
/// settings reload. Never fails the caller: autostart is a
/// convenience, and a home directory we cannot write to is not a
/// reason to refuse to run, so problems are logged and swallowed.
pub fn sync(enabled: bool, app: App<'_>) {
    imp::sync(enabled, app);
}
