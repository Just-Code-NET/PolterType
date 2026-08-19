//! Installing, verifying and removing plug-in language packs.
//!
//! The loader in [`crate::layouts::plugins`] reads
//! `<data_dir>/plugins/<id>/`; this is the supported way to get a pack
//! in there, with validation, instead of copying directories by hand.
//!
//! **There is no download.** `install` takes a directory already on the
//! user's disk; fetching is the browser's job. That is the security
//! boundary, not laziness — see `docs/ARCHITECTURE.md` § Plug-ins. It
//! also removes zip-slip, decompression bombs and half-extracted packs
//! at the root, because there is no archive.
//!
//! A pack is *data*, and the loader is built on that assumption, so
//! installation copies only what a data-only pack may contain: an
//! allow-list of names and extensions (no executable, no `.so`, no
//! `config.toml` shadowing the user's settings), no traversal and no
//! symlinks, a size budget, and atomic replacement so an interrupted
//! install leaves the old pack or none.

//! An **extension** ([`PluginKind::Extension`]) ships a program, spawned
//! as a separate process and never loaded into PolterType. What it
//! contributes to the UI is declared statically and rendered by
//! PolterType, so a plug-in never draws and can never imitate a system
//! prompt. [`check_extension`] enforces the parts a manifest could lie
//! about.

mod consts;
mod discover;
mod enums;
mod install;
mod settings;
mod types;
mod validate;

pub use consts::*;
pub use discover::*;
pub use enums::*;
pub use install::*;
pub use settings::*;
pub use types::*;
pub use validate::*;

#[cfg(test)]
mod tests;
