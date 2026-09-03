//! No mode bits to set: the file inherits the directory's ACL, and the
//! directory this is recommended for is under the user profile.

use std::path::Path;

use anyhow::{Context, Result};

pub(crate) fn write(path: &Path, contents: &str) -> Result<()> {
    std::fs::write(path, contents).with_context(|| format!("create {}", path.display()))
}
