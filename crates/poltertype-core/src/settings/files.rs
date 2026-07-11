//! Atomic on-disk writes for the settings file.

use super::*;
use std::fs;
use std::path::Path;

pub(crate) fn write_atomically(path: &Path, s: &Settings) -> Result<(), SettingsError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let serialized = toml::to_string_pretty(s)?;
    let tmp = path.with_extension("toml.tmp");
    fs::write(&tmp, serialized)?;
    fs::rename(&tmp, path)?;
    Ok(())
}
