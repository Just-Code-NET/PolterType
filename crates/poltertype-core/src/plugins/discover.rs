//! Finding plug-ins: the installed ones, plus whatever a developer is
//! working on.
//!
//! There are three sources, in order of precedence:
//!
//! 1. **Installed** — `<data_dir>/plugins/<id>/`, put there by
//!    [`super::install`]. This is what a user has.
//! 2. **`POLTERTYPE_PLUGIN_PATH`** — a `PATH`-style list of plug-in
//!    directories, honoured in every build. An explicit, visible way
//!    to run a plug-in that has not been installed, without pretending
//!    a debug build is the only place anyone tests.
//! 3. **Sibling checkouts, debug builds only** — the directories next
//!    to this repository. Working on a plug-in means `cargo run -p
//!    poltertype-app` should just find it; asking a developer to
//!    install their own working copy after every build is a step that
//!    exists only to be forgotten.
//!
//! Source 3 is `debug_assertions`-only on purpose. It is the one that
//! runs a program nobody explicitly pointed at, and "a directory
//! happened to be next to the checkout" is nowhere near a good enough
//! reason to do that in something a user installed.
//!
//! ## Where a developer's binary actually is
//!
//! An installed plug-in keeps its program in `bin/`. A checkout has it
//! in `target/debug/` or `target/release/`, because that is where
//! Cargo puts it. [`resolve_exe`] looks in all three, newest first
//! among the build directories, so a rebuild is picked up without
//! copying anything into place.

use std::path::{Path, PathBuf};

use tracing::{debug, warn};

use super::consts::{MANIFEST_NAME, PLUGINS_DIR};
use super::enums::{PluginError, PluginKind};
use super::install::read_header;
use super::types::ExtensionManifest;
use super::validate::{EXTENSION_BIN_DIR, check_extension};

/// Environment variable holding extra plug-in directories, separated
/// the way `PATH` is on this platform.
pub const PLUGIN_PATH_VAR: &str = "POLTERTYPE_PLUGIN_PATH";

/// An extension that was found, checked, and can be run.
#[derive(Debug, Clone)]
pub struct DiscoveredExtension {
    pub id: String,
    pub name: String,
    pub version: String,
    /// The plug-in's own directory.
    pub dir: PathBuf,
    /// The program to run, already resolved to a file that exists.
    pub exe: PathBuf,
    pub manifest: ExtensionManifest,
    /// True when this came from a sibling checkout or
    /// `POLTERTYPE_PLUGIN_PATH` rather than from an install. Worth
    /// showing in the UI: it is running code that was never installed.
    pub development: bool,
}

/// Every extension available right now, sorted by id, one entry per id
/// (an installed plug-in wins over a checkout of the same id, so a
/// developer's copy never silently shadows what the user installed
/// unless they asked for it by putting it on the path).
pub fn extensions(data_dir: &Path) -> Vec<DiscoveredExtension> {
    let mut extra = path_dirs();
    extra.extend(sibling_dirs());
    extensions_from(data_dir, &extra)
}

/// The same, with the development directories supplied rather than
/// discovered. Split out so the precedence rules can be tested without
/// reaching into the environment — which this crate could not do
/// anyway, since it forbids `unsafe` and `set_var` is unsafe.
pub fn extensions_from(data_dir: &Path, extra: &[PathBuf]) -> Vec<DiscoveredExtension> {
    let mut found: Vec<DiscoveredExtension> = Vec::new();

    for dir in installed_dirs(data_dir) {
        push_extension(&mut found, &dir, false);
    }
    for dir in extra {
        push_extension(&mut found, dir, true);
    }

    found.sort_by(|a, b| a.id.cmp(&b.id));
    found
}

fn push_extension(out: &mut Vec<DiscoveredExtension>, dir: &Path, development: bool) {
    match load(dir) {
        Ok(Some(mut ext)) => {
            ext.development = development;
            if out.iter().any(|e| e.id == ext.id) {
                debug!(id = %ext.id, "plug-in already found earlier; ignoring {}", dir.display());
                return;
            }
            debug!(id = %ext.id, development, path = %dir.display(), "extension available");
            out.push(ext);
        }
        Ok(None) => {}
        Err(e) => warn!("ignoring plug-in at {}: {e}", dir.display()),
    }
}

/// Read one directory as an extension. `Ok(None)` means "a valid
/// plug-in, but a data pack" — not something to complain about.
pub fn load(dir: &Path) -> Result<Option<DiscoveredExtension>, PluginError> {
    if !dir.join(MANIFEST_NAME).is_file() {
        return Ok(None);
    }
    let header = read_header(dir)?;
    if header.kind != PluginKind::Extension {
        return Ok(None);
    }
    check_extension(&header.extension)?;

    let exe = resolve_exe(dir, &header.extension.exe).ok_or_else(|| {
        PluginError::NoExecutable(format!(
            "{} declares {} but no built copy of it was found",
            MANIFEST_NAME, header.extension.exe
        ))
    })?;

    // The layout loader's view of the same file carries the identity.
    let identity = super::install::read_manifest(dir)?;
    Ok(Some(DiscoveredExtension {
        id: identity.id,
        name: identity.name,
        version: identity.version,
        dir: dir.to_path_buf(),
        exe,
        manifest: header.extension,
        development: false,
    }))
}

/// Where the program actually is: installed under `bin/`, or built by
/// Cargo into `target/`. Newest wins between debug and release, so
/// whichever was built last is the one that runs.
pub fn resolve_exe(dir: &Path, name: &str) -> Option<PathBuf> {
    let installed = dir.join(EXTENSION_BIN_DIR).join(name);
    if installed.is_file() {
        return Some(installed);
    }
    let mut built: Vec<(std::time::SystemTime, PathBuf)> = ["debug", "release"]
        .iter()
        .map(|profile| dir.join("target").join(profile).join(name))
        .filter(|p| p.is_file())
        .filter_map(|p| {
            let when = std::fs::metadata(&p).ok()?.modified().ok()?;
            Some((when, p))
        })
        .collect();
    built.sort_by_key(|(when, _)| std::cmp::Reverse(*when));
    built.into_iter().next().map(|(_, p)| p)
}

fn installed_dirs(data_dir: &Path) -> Vec<PathBuf> {
    let plugins = data_dir.join(PLUGINS_DIR);
    let Ok(entries) = std::fs::read_dir(&plugins) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| e.path())
        .collect()
}

fn path_dirs() -> Vec<PathBuf> {
    let Some(raw) = std::env::var_os(PLUGIN_PATH_VAR) else {
        return Vec::new();
    };
    std::env::split_paths(&raw).filter(|p| p.is_dir()).collect()
}

/// Directories beside this checkout, in debug builds only.
///
/// The workspace root is found from `CARGO_MANIFEST_DIR`, which is
/// baked in at compile time — so this is empty in any binary that was
/// not built from a source tree that still exists.
fn sibling_dirs() -> Vec<PathBuf> {
    if !cfg!(debug_assertions) {
        return Vec::new();
    }
    // <crate>/crates/poltertype-core → <crate> → the directory holding
    // this repository and its siblings.
    let Some(workspace) = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .and_then(|p| p.parent())
    else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(workspace) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .filter(|p| p.join(MANIFEST_NAME).is_file())
        .collect()
}

#[cfg(test)]
#[path = "discover_tests.rs"]
mod tests;
