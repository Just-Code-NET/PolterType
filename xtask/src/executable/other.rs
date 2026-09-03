//! No mode bits to restore off Unix.

use std::path::Path;

use anyhow::Result;

pub(crate) fn mark_all(_dir: &Path) -> Result<()> {
    Ok(())
}
