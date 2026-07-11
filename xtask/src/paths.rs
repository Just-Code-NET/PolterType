//! Repo-root discovery for the dev tooling.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub(crate) fn repo_root() -> Result<PathBuf> {
    // CARGO_MANIFEST_DIR for xtask = <root>/xtask; go up one.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .map(Path::to_path_buf)
        .context("xtask CARGO_MANIFEST_DIR has no parent")
}
