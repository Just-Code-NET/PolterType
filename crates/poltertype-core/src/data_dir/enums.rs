//! Data-directory resolution errors.

use super::*;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DataDirError {
    /// `current_exe()` failed — extremely rare (running from a
    /// deleted binary, locked-down sandbox).
    #[error("could not locate the running executable: {0}")]
    NoCurrentExe(#[from] std::io::Error),

    /// None of the candidate locations contained a usable data dir.
    /// `tried` lists every path the resolver considered, in
    /// preference order, so a misdeployed install is debuggable from
    /// a single log line.
    #[error(
        "poltertype data directory not found. Tried (in order): {}",
        format_tried(.tried)
    )]
    NotFound { tried: Vec<PathBuf> },
}
