//! Resolve the on-disk **data directory** that ships layout mappings,
//! FST wordlists, and (later) plug-ins.
//!
//! Layout on disk (relative to the resolved root):
//!
//! ```text
//! <data_dir>/
//!   layout-mappings/
//!     en_us.toml
//!     uk_ua.toml
//!     ...
//!   wordlists/
//!     en_us.fst                  ← built from data/wordlists/en_us.txt.gz
//!     en_us-stop.txt             ← curated 1- / 2-letter stop words
//!     ...
//!   plugins/                     ← reserved for the future plug-in
//!                                  marketplace; loader checks but
//!                                  does nothing today
//! ```
//!
//! Why externalised at all (vs. `include_bytes!`-baked):
//!
//! * Lets the app load **only** the wordlists for OS-active layouts
//!   instead of paying RAM for all six baked-in dictionaries — the
//!   user with `en-US / uk-UA / ru-RU` saves ~5–10 MB of FST
//!   memory by simply not opening fr-FR / es-ES / de-DE.
//! * Future-proofs the plug-in / language-pack story — third-party
//!   data drops next to the bundled set, no rebuild needed.
//! * Makes installers ship a shared `data/` tree instead of bloating
//!   the executable.
//!
//! Resolution order (first existing wins):
//!
//! 1. `KB_SWITCHER_DATA_DIR` env override — escape hatch for tests
//!    and unusual deployments.
//! 2. `<exe_dir>/data/` — Windows MSI install layout, portable mode,
//!    and the layout the AppImage `linuxdeploy` produces.
//! 3. `<exe_dir>/../Resources/data/` — macOS `.app` bundle layout.
//! 4. `<exe_dir>/../share/kb-switcher/data/` — alternate Linux layout
//!    when an unprefixed binary is dropped in `/usr/bin/`.
//! 5. `<workspace>/target/dist/data/` (deduced from `<exe_dir>` by
//!    walking up to a parent named `target`) — dev mode, where
//!    `kb-core/build.rs` writes prepared FSTs.
//!
//! If nothing matches, [`resolve`] returns
//! [`DataDirError::NotFound`] listing every path it tried so users
//! can fix the deployment.

use std::path::{Path, PathBuf};

use thiserror::Error;

/// Env var an operator (or a test) sets to pin the data dir
/// explicitly. Highest-priority lookup in [`resolve`].
pub const ENV_OVERRIDE: &str = "KB_SWITCHER_DATA_DIR";

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
        "kb-switcher data directory not found. Tried (in order): {}",
        format_tried(.tried)
    )]
    NotFound { tried: Vec<PathBuf> },
}

fn format_tried(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(" | ")
}

/// Resolve the data directory using the rules in the module-level doc.
///
/// The returned path is guaranteed to exist as a directory at
/// resolution time. We don't peek inside it — missing layout files
/// surface as graceful "no dictionary for this layout" warnings
/// later, the same way the embedded codepath used to handle empty
/// FSTs.
pub fn resolve() -> Result<PathBuf, DataDirError> {
    let mut tried: Vec<PathBuf> = Vec::new();

    // (1) explicit env override — wins outright if it points at a
    // real directory; we also include it in `tried` if it didn't, so
    // a typo'd path is visible in the error.
    if let Some(val) = env_override() {
        let path = PathBuf::from(&val);
        if is_dir(&path) {
            return Ok(canonical_or_self(path));
        }
        tried.push(path);
    }

    // Everything else is exe-relative. If we can't even locate the
    // exe we fail with a distinct error so the user sees what's wrong.
    let exe = std::env::current_exe()?;
    let exe_dir = exe.parent().unwrap_or(Path::new("")).to_path_buf();

    for candidate in candidates_relative_to_exe(&exe_dir) {
        if is_dir(&candidate) {
            return Ok(canonical_or_self(candidate));
        }
        tried.push(candidate);
    }

    Err(DataDirError::NotFound { tried })
}

/// Test seam — production code calls [`resolve`] which reads from
/// the actual environment. Tests build candidate lists deterministically.
fn env_override() -> Option<String> {
    std::env::var(ENV_OVERRIDE).ok().filter(|s| !s.is_empty())
}

fn is_dir(p: &Path) -> bool {
    std::fs::metadata(p).map(|m| m.is_dir()).unwrap_or(false)
}

/// Try to canonicalise; on failure (broken symlink, permission, etc.)
/// keep the original path so the caller still has *something*
/// useable. Canonicalisation is purely a "tidy log output / make
/// path unique" affair, not a correctness requirement.
fn canonical_or_self(p: PathBuf) -> PathBuf {
    std::fs::canonicalize(&p).unwrap_or(p)
}

/// Build the ordered list of candidate paths relative to a given
/// exe directory. Pulled out as a fn (and made `pub(crate)`) so the
/// unit tests below can drive it without touching `current_exe`.
fn candidates_relative_to_exe(exe_dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![
        // (2) Windows MSI / portable / linuxdeploy AppImage AppDir.
        exe_dir.join("data"),
        // (3) macOS .app — exe lives in Contents/MacOS, data goes in
        // Contents/Resources per Apple convention.
        exe_dir.join("..").join("Resources").join("data"),
        // (4) FHS-style Linux: /usr/bin/kb-switcher, data in
        // /usr/share/kb-switcher.
        exe_dir
            .join("..")
            .join("share")
            .join("kb-switcher")
            .join("data"),
    ];

    // (5) dev-tree fallback. From `target/<profile>/kb-app[.exe]`
    // walk up to find the `target` dir, then drop into
    // `target/dist/data` where build.rs writes the prepared assets.
    if let Some(target) = find_ancestor_named(exe_dir, "target") {
        out.push(target.join("dist").join("data"));
    }

    out
}

/// Walk parents of `start` until we find a directory whose final
/// component equals `name`. Returns the matching ancestor, or `None`
/// if none of `start`'s parents qualify.
///
/// We tolerate the case where `start` itself has the matching name
/// (rare but possible if cargo's target dir is named oddly) by
/// checking the input first.
fn find_ancestor_named(start: &Path, name: &str) -> Option<PathBuf> {
    for ancestor in start.ancestors() {
        if ancestor.file_name().is_some_and(|n| n == name) {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

#[cfg(test)]
mod tests {
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
            cs[2].ends_with(Path::new("share/kb-switcher/data"))
                || cs[2].ends_with("share\\kb-switcher\\data")
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
            cs.last()
                .is_some_and(|p| p.ends_with(Path::new("target/dist/data"))
                    || p.ends_with("target\\dist\\data")),
            "expected dev-fallback last; got {cs:?}"
        );
    }

    /// `target/release/kb-app` is the same shape as debug — both
    /// must surface the dev fallback. Otherwise `cargo build
    /// --release && target/release/kb-app` wouldn't find data.
    #[test]
    fn dev_fallback_works_in_release_profile_too() {
        let exe_dir = PathBuf::from("/repo/target/release");
        let cs = candidates_relative_to_exe(&exe_dir);
        assert!(cs.iter().any(
            |p| p.ends_with(Path::new("target/dist/data")) || p.ends_with("target\\dist\\data")
        ));
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
}
