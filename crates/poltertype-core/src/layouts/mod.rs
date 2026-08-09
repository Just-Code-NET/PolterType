//! Layout mappings — what each Win-SC1 scancode produces under each
//! supported layout. Loaded at runtime from the data directory resolved
//! by [`crate::data_dir`]; `docs/DATA_LAYOUT.md` has the tree, which
//! `build.rs` writes and the installers copy. An FST is never
//! re-derived at runtime, only `mmap`ed.
//!
//! [`LayoutDb::load`] takes an optional **active filter**, typically
//! `LayoutSwitcher::list_active()`. Only matching layouts are read into
//! memory, which saves a user with three enabled layouts the ~7-15 MB
//! of FST data for the ones they would never query.
//!
//! [`LoadOptions::os_keymaps`] carries what the platform backend says
//! the user's keyboards actually produce. Where a language has more
//! than one keyboard — Windows ships three Bulgarian ones under the
//! single id `bg-BG` — a bundled table can only be right for one, so
//! the OS's answer replaces it. See [`os_keymap`] for the precedence
//! chain and what it deliberately does not fix.
//!
//! Two user override paths layer on top: `<config-dir>/poltertype/
//! layouts/*.toml` adds whole layouts without a rebuild, and
//! `wordlists/<stem>(-extras|-stop).txt` extends any layout's
//! dictionary or short-stop list at runtime.

mod consts;
mod db;
mod enums;
mod files;
mod helpers;
mod os_keymap;
mod plugins;
mod types;

pub use db::LayoutDb;
pub use enums::LayoutLoadError;
pub use files::{user_layout_dir, user_profile_wordlist_dir, user_wordlist_dir};
pub use types::{LayoutMapping, LoadOptions, PluginManifest};

#[cfg(test)]
mod tests;
