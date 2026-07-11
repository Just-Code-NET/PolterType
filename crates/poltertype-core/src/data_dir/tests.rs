use std::path::{Path, PathBuf};

use super::*;

/// Sanity: each platform gets a distinct candidate. The order
/// matters — Windows-shaped installs should resolve before macOS-
/// shaped ones, otherwise a stray `Resources/data` somewhere up
/// the tree could shadow the real install.
#[test]
fn candidates_have_expected_shape() {
    let exe_dir = Path::new("/opt/kb/bin");
    let cs = candidates_relative_to_exe(exe_dir);
    // First three are platform-shaped (always present).
    assert!(cs[0].ends_with("data"));
    assert!(cs[1].ends_with(Path::new("Resources/data")) || cs[1].ends_with("Resources\\data"));
    assert!(
        cs[2].ends_with(Path::new("share/poltertype/data"))
            || cs[2].ends_with("share\\poltertype\\data")
    );
    // No `target` ancestor of `/opt/kb/bin`, so no dev fallback.
    assert_eq!(cs.len(), 3);
}

/// Dev mode: an exe under `…/target/debug/` should produce a
/// `…/target/dist/data` candidate as the dev fallback. Without
/// this `cargo run` would refuse to find data sitting in the
/// repo's target dir.
#[test]
fn dev_fallback_appears_under_target() {
    let exe_dir = PathBuf::from("/repo/target/debug");
    let cs = candidates_relative_to_exe(&exe_dir);
    assert!(
        cs.last().is_some_and(
            |p| p.ends_with(Path::new("target/dist/data")) || p.ends_with("target\\dist\\data")
        ),
        "expected dev-fallback last; got {cs:?}"
    );
}

/// `target/release/poltertype-app` is the same shape as debug — both
/// must surface the dev fallback. Otherwise `cargo build
/// --release && target/release/poltertype-app` wouldn't find data.
#[test]
fn dev_fallback_works_in_release_profile_too() {
    let exe_dir = PathBuf::from("/repo/target/release");
    let cs = candidates_relative_to_exe(&exe_dir);
    assert!(
        cs.iter().any(
            |p| p.ends_with(Path::new("target/dist/data")) || p.ends_with("target\\dist\\data")
        )
    );
}

/// Production-shaped path (no `target` ancestor) → no dev
/// fallback in the candidate list. Avoids resolver races where a
/// stray `target` dir under an install root would be mistaken
/// for a dev workspace.
#[test]
fn no_dev_fallback_when_no_target_ancestor() {
    let exe_dir = Path::new("/usr/local/bin");
    let cs = candidates_relative_to_exe(exe_dir);
    assert!(
        cs.iter().all(|p| !p.to_string_lossy().contains("target")),
        "production exe path must not synthesise a dev fallback: {cs:?}"
    );
}

#[test]
fn find_ancestor_finds_named_parent() {
    let p = Path::new("/a/b/target/debug/foo");
    assert_eq!(
        find_ancestor_named(p, "target"),
        Some(PathBuf::from("/a/b/target"))
    );
}

#[test]
fn find_ancestor_returns_none_when_absent() {
    let p = Path::new("/a/b/c");
    assert_eq!(find_ancestor_named(p, "target"), None);
}
