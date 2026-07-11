//! Resolve the on-disk **data directory** that ships layout mappings,
//! FST wordlists, and (later) plug-ins.
//!
//! Layout on disk (relative to the resolved root):
//!
//! ```text
//! <data_dir>/
//!   layout-mappings/
//!     en_us.toml
//!     uk_ua.toml
//!     ...
//!   wordlists/
//!     en_us.fst                  ← built from data/wordlists/en_us.txt.gz
//!     en_us-stop.txt             ← curated 1- / 2-letter stop words
//!     ...
//!   plugins/                     ← reserved for the future plug-in
//!                                  marketplace; loader checks but
//!                                  does nothing today
//! ```
//!
//! Why externalised at all (vs. `include_bytes!`-baked):
//!
//! * Lets the app load **only** the wordlists for OS-active layouts
//!   instead of paying RAM for all six baked-in dictionaries — the
//!   user with `en-US / uk-UA / ru-RU` saves ~5–10 MB of FST
//!   memory by simply not opening fr-FR / es-ES / de-DE.
//! * Future-proofs the plug-in / language-pack story — third-party
//!   data drops next to the bundled set, no rebuild needed.
//! * Makes installers ship a shared `data/` tree instead of bloating
//!   the executable.
//!
//! Resolution order (first existing wins):
//!
//! 1. `POLTERTYPE_DATA_DIR` env override — escape hatch for tests
//!    and unusual deployments.
//! 2. `<exe_dir>/data/` — Windows MSI install layout, portable mode,
//!    and the layout the AppImage `linuxdeploy` produces.
//! 3. `<exe_dir>/../Resources/data/` — macOS `.app` bundle layout.
//! 4. `<exe_dir>/../share/poltertype/data/` — alternate Linux layout
//!    when an unprefixed binary is dropped in `/usr/bin/`.
//! 5. `<workspace>/target/dist/data/` (deduced from `<exe_dir>` by
//!    walking up to a parent named `target`) — dev mode, where
//!    `poltertype-core/build.rs` writes prepared FSTs.
//!
//! If nothing matches, [`resolve`] returns
//! [`DataDirError::NotFound`] listing every path it tried so users
//! can fix the deployment.

mod consts;
mod enums;
mod resolve;

pub use consts::*;
pub use enums::*;
pub use resolve::*;

#[cfg(test)]
mod tests;
