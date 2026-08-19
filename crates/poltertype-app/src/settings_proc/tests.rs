//! Unit tests for locating our own binary across "the file moved under
//! us" cases.

use std::path::{Path, PathBuf};

use super::enums::*;
use super::exe::*;

/// Filesystem stub: only the listed paths exist.
fn only(paths: &[&str]) -> impl Fn(&Path) -> bool + use<> {
    let set: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
    move |p: &Path| set.iter().any(|e| e == p)
}

#[test]
fn live_binary_is_used_as_is() {
    let raw = PathBuf::from("/usr/bin/poltertype");
    assert_eq!(
        classify(raw.clone(), only(&["/usr/bin/poltertype"])),
        OwnExe::Live(raw)
    );
}

#[test]
fn deleted_suffix_is_stripped_when_a_new_binary_took_the_place() {
    // Dev rebuild / in-place upgrade: a fresh binary sits at the path
    // `/proc/self/exe` still resolves to, minus the annotation.
    let raw = PathBuf::from("/usr/bin/poltertype (deleted)");
    assert_eq!(
        classify(raw, only(&["/usr/bin/poltertype"])),
        OwnExe::Replaced(PathBuf::from("/usr/bin/poltertype"))
    );
}

#[test]
fn deleted_with_nothing_at_the_real_path_is_gone() {
    // Uninstall / `cargo clean`: there is nothing left to spawn.
    let raw = PathBuf::from("/usr/bin/poltertype (deleted)");
    assert_eq!(classify(raw.clone(), only(&[])), OwnExe::Gone(raw));
}

#[test]
fn a_file_genuinely_named_deleted_is_not_rewritten() {
    // ` (deleted)` is a legal file name. If the path exists, it is us,
    // and we must not "helpfully" strip it and launch a neighbour.
    let raw = PathBuf::from("/opt/poltertype (deleted)");
    assert_eq!(
        classify(
            raw.clone(),
            only(&["/opt/poltertype (deleted)", "/opt/poltertype"])
        ),
        OwnExe::Live(raw)
    );
}

#[test]
fn a_missing_path_without_the_suffix_is_gone() {
    let raw = PathBuf::from("/usr/bin/poltertype");
    assert_eq!(classify(raw.clone(), only(&[])), OwnExe::Gone(raw));
}
