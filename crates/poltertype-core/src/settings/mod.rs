//! Persistent settings stored as TOML in the platform's standard
//! per-user config dir.
//!
//! Path:
//!   * Windows : `%APPDATA%\poltertype\config.toml`
//!   * macOS   : `~/Library/Application Support/poltertype/config.toml`
//!   * Linux   : `$XDG_CONFIG_HOME/poltertype/config.toml`
//!
//! Schema is versioned (`schema_version`) so future migrations can
//! tell old configs apart without breaking the user's file.

mod consts;
mod enums;
mod files;
mod store;
mod types;

pub(crate) use consts::*;
pub use enums::*;
pub(crate) use files::*;
pub use store::*;
pub use types::*;

#[cfg(test)]
mod tests;
