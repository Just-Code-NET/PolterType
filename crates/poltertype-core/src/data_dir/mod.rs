//! Resolve the on-disk **data directory** holding layout mappings, FST
//! wordlists and plug-ins. `docs/DATA_LAYOUT.md` has the tree.
//!
//! Externalised rather than `include_bytes!`-baked so the app can load
//! only the wordlists for OS-active layouts instead of paying RAM for
//! every bundled dictionary, so third-party data can drop next to the
//! bundled set without a rebuild, and so installers ship a shared
//! `data/` tree rather than a bloated executable.
//!
//! Resolution order, first existing wins:
//!
//! 1. `POLTERTYPE_DATA_DIR` — escape hatch for tests and unusual
//!    deployments.
//! 2. `<exe_dir>/data/` — Windows MSI, portable mode, AppImage.
//! 3. `<exe_dir>/../Resources/data/` — macOS `.app` bundle.
//! 4. `<exe_dir>/../share/poltertype/data/` — unprefixed Linux binary.
//! 5. `<workspace>/target/dist/data/` — dev mode, where
//!    `poltertype-core/build.rs` writes prepared FSTs.
//!
//! With no match, [`resolve`] returns [`DataDirError::NotFound`]
//! listing every path it tried.

mod consts;
mod enums;
mod resolve;

pub use consts::*;
pub use enums::*;
pub use resolve::*;

#[cfg(test)]
mod tests;
