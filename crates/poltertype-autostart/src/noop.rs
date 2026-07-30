//! Platforms with no autostart backend.
//!
//! Reached only on targets outside the three we ship. `config.toml`
//! keeps the setting either way, so nothing is lost if a backend
//! appears later.

use crate::types::App;

pub(crate) fn sync(_enabled: bool, _app: App<'_>) {}
