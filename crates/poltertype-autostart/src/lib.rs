//! Make the OS start PolterType at login, or stop it.
//!
//! | Platform | Mechanism | Entry |
//! |---|---|---|
//! | macOS | per-user LaunchAgent + `launchctl` | `~/Library/LaunchAgents/<id>.plist` |
//! | Windows | `HKCU` run key via `reg.exe` | `…\CurrentVersion\Run`, value `<id>` |
//! | Linux | systemd user service, or an XDG entry with no user manager | `$XDG_CONFIG_HOME/systemd/user/<id>.service` |
//! | other | noop | — |
//!
//! One of the platform-code islands (see `CONTRIBUTING.md`), and
//! deliberately *not* a trait + factory like `poltertype-layout`: there
//! is no backend for the caller to hold, just one idempotent operation.
//!
//! Each backend drives the mechanism the OS already ships — two
//! `launchctl` calls, one `reg.exe` call, a `systemctl --user` call or
//! a file write — which buys no per-OS dependency and
//! `#![forbid(unsafe_code)]`, at the cost of a short-lived process at
//! most twice per app start.
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
/// Idempotent and safe to call often. Never fails the caller — a home
/// directory we cannot write to is no reason to refuse to run, so
/// problems are logged and swallowed.
pub use imp::sync;
