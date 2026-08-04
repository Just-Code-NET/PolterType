//! Installing, verifying and removing plug-in language packs.
//!
//! The loader in [`crate::layouts::plugins`] has read
//! `<data_dir>/plugins/<id>/` since v0.1; what was missing was any
//! supported way to *get* a pack in there. People copied directories
//! by hand, which meant no validation ran and a malformed pack showed
//! up as a puzzling startup warning.
//!
//! ## What this deliberately is not
//!
//! **There is no download.** `install` takes a directory that already
//! exists on the user's disk; fetching a pack is the browser's job.
//!
//! That is not laziness, it is the security boundary. PolterType reads
//! every keystroke, and its one network call today is an updater that
//! sends nothing and verifies a signature made by a key that never
//! touches CI. Adding "and also fetches arbitrary third-party content
//! from a URL in a config file" would be a second, much wider channel
//! guarding data that lands in the same process. A user who has
//! already downloaded a pack has made the trust decision explicitly,
//! at a moment when they can see what they are downloading — which is
//! exactly where that decision belongs.
//!
//! It also removes a whole class of bug at the root: there is no
//! archive to unpack, so there is no zip-slip, no decompression bomb,
//! and no partially-extracted pack to clean up.
//!
//! ## What `install` actually guards against
//!
//! A pack is *data* — layout TOMLs and dictionaries — and the loader
//! is built on that assumption. So installation copies only what a
//! data-only pack is allowed to contain, rather than copying a
//! directory and hoping:
//!
//! * **An allow-list of names and extensions.** Anything else in the
//!   source directory is reported and left behind. A pack cannot
//!   deliver an executable, a `.so`, a dotfile, or a `config.toml`
//!   that would shadow the user's settings.
//! * **No traversal, no links.** Every destination path is checked to
//!   be inside the pack directory, and symlinks are refused rather
//!   than followed — a symlink named `layout-mappings` pointing at
//!   `~/.ssh` must not become a readable copy.
//! * **A size budget**, so a "language pack" cannot quietly fill the
//!   user's disk.
//! * **Atomic replacement.** The pack is staged beside its final
//!   location and renamed into place, so an interrupted install
//!   leaves either the old pack or none — never half of a new one.

//! ## Extensions are a second kind, not a looser pack
//!
//! Everything above describes a *language pack*, and none of it
//! changes. A plug-in that ships a program is a different kind
//! ([`PluginKind::Extension`]), declared as such in its manifest, so
//! the larger decision the user is making is visible before anything
//! is installed rather than inferred from what a directory happens to
//! contain.
//!
//! An extension is never loaded into PolterType. It is a separate
//! program that PolterType spawns, which is what keeps a third-party
//! crash — or a third-party compromise — away from the process holding
//! the global keyboard hook. What it contributes to the UI is
//! *declared statically* in the manifest and rendered by PolterType
//! itself: a plug-in never draws, so it can never imitate a system
//! prompt or PolterType's own dialogs. [`check_extension`] is what
//! enforces the parts of that a manifest could otherwise lie about.

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
