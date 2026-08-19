//! The resolution walk: env override → exe-relative → dev tree.

use super::*;
use std::path::{Path, PathBuf};

pub(crate) fn format_tried(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(" | ")
}

/// Resolve the data directory using the rules in the module-level doc.
///
/// The returned path is guaranteed to exist as a directory at
/// resolution time; its contents are not checked. Missing layout files
/// surface later as "no dictionary for this layout" warnings.
pub fn resolve() -> Result<PathBuf, DataDirError> {
    let mut tried: Vec<PathBuf> = Vec::new();

    // (1) explicit env override. Pushed onto `tried` when it is not a
    // directory, so a typo'd path is visible in the error.
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
pub(crate) fn env_override() -> Option<String> {
    std::env::var(ENV_OVERRIDE).ok().filter(|s| !s.is_empty())
}

pub(crate) fn is_dir(p: &Path) -> bool {
    std::fs::metadata(p).map(|m| m.is_dir()).unwrap_or(false)
}

/// Try to canonicalise, keeping the original path on failure (broken
/// symlink, permissions): canonicalisation only tidies log output, it
/// is not a correctness requirement.
pub(crate) fn canonical_or_self(p: PathBuf) -> PathBuf {
    std::fs::canonicalize(&p).unwrap_or(p)
}

/// Build the ordered list of candidate paths relative to a given exe
/// directory. Separate from [`resolve`] so tests can drive it without
/// touching `current_exe`.
pub(crate) fn candidates_relative_to_exe(exe_dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![
        // (2) Windows MSI / portable / linuxdeploy AppImage AppDir.
        exe_dir.join("data"),
        // (3) macOS .app — exe lives in Contents/MacOS, data goes in
        // Contents/Resources per Apple convention.
        exe_dir.join("..").join("Resources").join("data"),
        // (4) FHS-style Linux: /usr/bin/poltertype, data in
        // /usr/share/poltertype.
        exe_dir
            .join("..")
            .join("share")
            .join("poltertype")
            .join("data"),
    ];

    // (5) dev-tree fallback. From `target/<profile>/poltertype-app[.exe]`
    // walk up to find the `target` dir, then drop into
    // `target/dist/data` where build.rs writes the prepared assets.
    if let Some(target) = find_ancestor_named(exe_dir, "target") {
        out.push(target.join("dist").join("data"));
    }

    out
}

/// The nearest ancestor of `start` — `start` itself included — whose
/// final component equals `name`.
pub(crate) fn find_ancestor_named(start: &Path, name: &str) -> Option<PathBuf> {
    for ancestor in start.ancestors() {
        if ancestor.file_name().is_some_and(|n| n == name) {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}
