//! Repo-root discovery for the dev tooling.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub(crate) fn repo_root() -> Result<PathBuf> {
    // CARGO_MANIFEST_DIR for xtask = <root>/xtask; go up one. Read at
    // runtime (cargo sets it for `cargo run`/`cargo xtask`) — the
    // `env!` macro would freeze the path of whatever checkout the
    // cached xtask binary was compiled in.
    let manifest = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").context("CARGO_MANIFEST_DIR not set")?,
    );
    manifest
        .parent()
        .map(Path::to_path_buf)
        .context("xtask CARGO_MANIFEST_DIR has no parent")
}
