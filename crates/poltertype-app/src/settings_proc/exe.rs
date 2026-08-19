//! Locating our own binary so we can spawn a second copy of it.

use std::path::{Path, PathBuf};

use super::consts::*;
use super::enums::*;

/// Resolve the binary this process was started from, classifying the
/// "someone changed it under us" cases (see [`OwnExe`]).
///
/// `Err` only when the OS won't tell us where we are at all — on Linux
/// that means `/proc` isn't mounted, which is exotic enough that the
/// caller just reports it.
pub(super) fn resolve_own_exe() -> std::io::Result<OwnExe> {
    std::env::current_exe().map(|raw| classify(raw, |p| p.exists()))
}

/// The decision itself, with the filesystem behind a predicate so the
/// tests can pin every branch without touching disk.
pub(super) fn classify(raw: PathBuf, exists: impl Fn(&Path) -> bool) -> OwnExe {
    // Existence first, always. A file genuinely named `foo (deleted)`
    // is legal, and if it is there, it is us — no rewriting.
    if exists(&raw) {
        return OwnExe::Live(raw);
    }
    match strip_deleted_suffix(&raw) {
        // A rebuilt dev binary or an in-place upgrade now sits at the
        // old path — launching it beats doing nothing at all.
        Some(real) if exists(&real) => OwnExe::Replaced(real),
        _ => OwnExe::Gone(raw),
    }
}

/// Recover the path our binary lived at; see [`DELETED_SUFFIX`].
fn strip_deleted_suffix(raw: &Path) -> Option<PathBuf> {
    let name = raw.file_name()?.to_str()?;
    let real = name.strip_suffix(DELETED_SUFFIX)?;
    Some(raw.with_file_name(real))
}
