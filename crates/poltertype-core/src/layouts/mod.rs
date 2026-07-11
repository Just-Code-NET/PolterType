//! Layout mappings — what each Win-SC1 scancode produces under each
//! supported layout. Loaded at runtime from the on-disk **data
//! directory** resolved by [`crate::data_dir`].
//!
//! ## On-disk layout
//!
//! ```text
//! <data_dir>/
//!   layout-mappings/<stem>.toml         ← mapping table
//!   wordlists/<stem>.fst                ← FST built from <stem>.txt.gz
//!   wordlists/<stem>-stop.txt           ← curated 1- / 2-letter words
//! ```
//!
//! `build.rs` writes this tree to `<workspace>/target/dist/data/` from
//! the committed sources under `data/`. Installers copy the same tree
//! next to the executable. At runtime we never re-derive an FST — we
//! just `mmap` the prepared `.fst` files.
//!
//! ## Active-layout filter
//!
//! [`LayoutDb::load`] takes an optional **active filter** — typically
//! the list returned by `LayoutSwitcher::list_active()`. When set, only
//! layouts whose `id` matches are read into memory; the others stay on
//! disk. A user with `en-US / uk-UA / ru-RU` enabled in the OS skips
//! loading ~7-15 MB of fr-FR / es-ES / de-DE FST data they'd never
//! query.
//!
//! ## User extensions
//!
//! Two override paths layered on top of the bundled set:
//!
//! 1. `<config-dir>/poltertype/layouts/*.toml` — add new layouts
//!    without rebuilding. Same TOML schema as the bundled ones.
//! 2. `<config-dir>/poltertype/wordlists/<stem>(-extras|-stop).txt`
//!    — extend the dictionary or short-stop list of any layout
//!    (bundled or user) at runtime.

mod consts;
mod db;
mod enums;
mod files;
mod helpers;
mod plugins;
mod types;

pub use db::LayoutDb;
pub use enums::LayoutLoadError;
pub use files::{user_layout_dir, user_profile_wordlist_dir, user_wordlist_dir};
pub use types::{LayoutMapping, LoadOptions};

#[cfg(test)]
mod tests;
