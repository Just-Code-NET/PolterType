//! Persistent settings, stored as TOML in the platform's per-user
//! config directory:
//!
//! * Windows — `%APPDATA%\poltertype\config.toml`
//! * macOS — `~/Library/Application Support/poltertype/config.toml`
//! * Linux — `$XDG_CONFIG_HOME/poltertype/config.toml`
//!
//! The schema is versioned so migrations can tell old configs apart
//! without breaking the user's file.

mod consts;
mod enums;
mod files;
mod migrate;
mod store;
mod types;

pub(crate) use consts::*;
// `consts` is otherwise crate-internal, but this one constant is part
// of the public API other crates already depend on.
pub use consts::MIN_UPDATE_INTERVAL_HOURS;
pub use enums::*;
pub(crate) use files::*;
pub(crate) use migrate::*;
pub use store::*;
pub use types::*;

#[cfg(test)]
mod tests;
