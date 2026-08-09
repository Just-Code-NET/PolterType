//! Installing, verifying and removing plug-in language packs.
//!
//! The loader in [`crate::layouts::plugins`] has read
//! `<data_dir>/plugins/<id>/` since v0.1; what was missing was a
//! supported way to *get* a pack in there, so people copied directories
//! by hand and no validation ran.
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

//! An **extension** ([`PluginKind::Extension`]) is a second kind rather
//! than a looser pack: it ships a program, and declaring that in the
//! manifest makes the larger decision visible before anything is
//! installed. It is never loaded into PolterType — it is spawned as a
//! separate program — and what it contributes to the UI is declared
//! statically and rendered by PolterType, so a plug-in never draws and
//! can never imitate a system prompt. [`check_extension`] enforces the
//! parts a manifest could lie about.

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
