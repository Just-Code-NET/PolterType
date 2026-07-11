//! Per-application wordlist profiles.
//!
//! ## What problem this solves
//!
//! The base wordlist overlay
//! `<config-dir>/poltertype/wordlists/<stem>.txt` is **global**:
//! every word added there boosts that layout's dictionary across
//! every application the user types into. That's right for
//! everyday vocabulary (family names, regional spellings) but
//! wrong for context-specific jargon — adding `kubectl` /
//! `terraform` to your en-US overlay so it stops flagging them in
//! VS Code also stops it from flagging them in chat, where you
//! actually do mean to type the English word.
//!
//! Profiles let the user keep separate overlay sets per context.
//! The engine swaps the active overlay when the foreground app
//! changes — `kubectl` only counts toward "this is English" while
//! the user is typing inside a code editor.
//!
//! ## On-disk layout
//!
//! Adds one directory level on top of the existing user-overlay
//! contract documented in `crates/poltertype-core/src/layouts.rs`:
//!
//! ```text
//! <config-dir>/poltertype/wordlists/
//!   <stem>.txt                  ← global overlay (existing — fallback)
//!   <stem>-stop.txt             ← global stop list (existing — fallback)
//!   profiles/
//!     <profile-id>/
//!       <stem>.txt              ← profile-specific overlay
//!       <stem>-stop.txt         ← profile-specific stop list
//! ```
//!
//! Same parser as the global overlay (`one word per line, # for
//! comments, blank lines ignored`), so power users can `cp` files
//! between the two layers and they'll just work.
//!
//! ## Schema
//!
//! See [`WordlistSettings`] / [`WordlistProfile`] below. Profiles
//! are matched by `apps` against the focused process's exe basename
//! (case-insensitive — same comparison
//! [`crate::settings::ExceptionSettings`] uses for `disabled_apps`,
//! so users only learn one matching rule).
//!
//! ## What's intentionally not here in v1
//!
//! * **Profile inheritance.** A profile is its own overlay set;
//!   it doesn't merge with the global overlay or another profile.
//!   Inheritance was tempting but adds load-time complexity (cycle
//!   detection, depth limit) and a UX surface ("which profile
//!   wins?") that isn't worth it until users actually ask for it.
//! * **Per-language vs per-layout profile granularity.** A profile
//!   has one `<stem>.txt` per layout, full stop. We considered
//!   "this profile only changes en-US" but the file system already
//!   gives you that — just don't create the other `<stem>.txt`
//!   files for the layouts you don't want to touch.
//! * **Hot reload.** Same constraint as the global overlay: the
//!   loader runs once at engine start. Editing files at runtime
//!   needs a tray restart. The Settings UI's banner spells this out.

mod resolve;
mod types;

pub use resolve::*;
pub use types::*;

#[cfg(test)]
mod tests;
