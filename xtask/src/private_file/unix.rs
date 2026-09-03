//! `O_CREAT|O_EXCL` at mode 0600.

use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use anyhow::{Context, Result};

/// Create `path` owner-only and fill it. Fails if it already exists —
/// a signing key is never silently replaced.
pub(crate) fn write(path: &Path, contents: &str) -> Result<()> {
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("create {}", path.display()))?;
    f.write_all(contents.as_bytes())?;
    Ok(())
}
