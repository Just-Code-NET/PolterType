//! Make the OS start PolterType at login, or stop it.
//!
//! | Platform | Mechanism | Entry |
//! |---|---|---|
//! | macOS | per-user LaunchAgent + `launchctl` | `~/Library/LaunchAgents/<id>.plist` |
//! | Windows | `HKCU` run key via `reg.exe` | `…\CurrentVersion\Run`, value `<id>` |
//! | Linux | XDG autostart entry | `$XDG_CONFIG_HOME/autostart/<id>.desktop` |
//! | other | noop | — |
//!
//! One of the platform-code islands (see `CONTRIBUTING.md`), and
//! deliberately *not* a trait + factory like `poltertype-layout`: there
//! is no backend for the caller to hold, just one idempotent operation.
//!
//! Each backend drives the mechanism the OS already ships — two
//! `launchctl` calls, one `reg.exe` call, or a file write — which buys
//! a crate with no per-OS dependency and `#![forbid(unsafe_code)]`, at
//! the cost of a short-lived process at most twice per app start. On
//! Windows the child is spawned with `CREATE_NO_WINDOW`, or a console
//! would flash at every login.
//!
//! It never *reads* the OS to discover intent: `config.toml` is the
//! single source of truth and [`sync`] only pushes it outwards. Delete
//! the LaunchAgent by hand and the next launch puts it back.

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
