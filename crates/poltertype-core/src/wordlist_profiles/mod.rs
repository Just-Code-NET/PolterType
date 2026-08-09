//! Per-application wordlist profiles.
//!
//! The base overlay `<config-dir>/poltertype/wordlists/<stem>.txt` is
//! **global**, which is right for everyday vocabulary and wrong for
//! context-specific jargon: adding `kubectl` so it stops being flagged
//! in VS Code also stops it being flagged in chat. Profiles keep
//! separate overlay sets per context, and the engine swaps the active
//! one when the foreground app changes.
//!
//! On disk this is one directory level on top of the existing overlay
//! contract — `wordlists/profiles/<profile-id>/<stem>.txt` — parsed by
//! the same parser, so files can be `cp`d between the two layers.
//! Profiles match on `apps` against the focused executable's basename,
//! case-insensitively, the same rule `disabled_apps` uses.
//!
//! Deliberately absent: **profile inheritance** (a profile is its own
//! set; cycle detection and "which profile wins?" are not worth it
//! until asked for), **per-language granularity** (the file system
//! already gives it — omit the `<stem>.txt` files you do not want), and
//! **hot reload** (the loader runs once at engine start; the Settings
//! UI banner says so).

mod resolve;
mod types;

pub use resolve::*;
pub use types::*;

#[cfg(test)]
mod tests;
