//! Install, list and remove packs.

use std::path::{Component, Path, PathBuf};

use tracing::{info, warn};

use super::consts::*;
use super::enums::{PluginError, PluginKind};
use super::types::{InstalledPack, ManifestHeader};
use super::validate::{EXTENSION_BIN_DIR, check_extension};
use crate::layouts::PluginManifest;

/// Install the pack in `src` into `<data_dir>/plugins/<id>/`.
///
/// Replaces an existing pack of the same id — the update path is
/// deliberately this same code, so it cannot be tested half as often.
///
/// Staged in a sibling directory and renamed into place, so an
/// interrupted install leaves the previous pack intact.
pub fn install(src: &Path, data_dir: &Path) -> Result<InstalledPack, PluginError> {
    if !src.is_dir() {
        return Err(PluginError::NotADirectory(src.to_path_buf()));
    }
    let manifest = read_manifest(src)?;
    let id = manifest.id.trim().to_owned();
    if id.is_empty() {
        return Err(PluginError::MissingId);
    }
    if !is_safe_id(&id) {
        return Err(PluginError::UnsafeId(id));
    }

    // What the plug-in claims to be decides what it may carry, so it is
    // read and validated before a single byte is copied.
    let header = read_header(src)?;
    if header.kind == PluginKind::Extension {
        check_extension(&header.extension)?;
        // Through `exe_in`: the manifest names the program without an
        // extension, so demanding an exact file name would make a
        // portable manifest uninstallable on Windows.
        if super::discover::exe_in(&src.join(EXTENSION_BIN_DIR), &header.extension.exe).is_none() {
            return Err(PluginError::NoExecutable(format!(
                "{} declares {}/{} but there is no such file",
                MANIFEST_NAME, EXTENSION_BIN_DIR, header.extension.exe
            )));
        }
    }

    let plan = plan_copy(src, header.kind)?;
    if plan.content_files == 0 {
        return Err(PluginError::Empty);
    }

    let plugins_dir = data_dir.join(PLUGINS_DIR);
    std::fs::create_dir_all(&plugins_dir)
        .map_err(|e| PluginError::io(format!("create {}", plugins_dir.display()), e))?;

    let dest = plugins_dir.join(&id);
    // Staged beside the destination so the rename is on one
    // filesystem — a cross-device rename would fall back to a copy
    // and lose the atomicity this exists for.
    let staging = plugins_dir.join(format!(".{id}.incoming"));
    let _ = std::fs::remove_dir_all(&staging);

    let copied = copy_plan(src, &staging, &plan).inspect_err(|_| {
        // Never leave a half-written staging directory behind.
        let _ = std::fs::remove_dir_all(&staging);
    })?;

    let replaced = dest.exists();
    if replaced {
        // Move the old pack aside rather than deleting it first, so a
        // failure between the two renames still leaves something
        // loadable on disk.
        let previous = plugins_dir.join(format!(".{id}.previous"));
        let _ = std::fs::remove_dir_all(&previous);
        std::fs::rename(&dest, &previous)
            .map_err(|e| PluginError::io(format!("move aside {}", dest.display()), e))?;
        if let Err(e) = std::fs::rename(&staging, &dest) {
            // Put the old one back before reporting.
            let _ = std::fs::rename(&previous, &dest);
            let _ = std::fs::remove_dir_all(&staging);
            return Err(PluginError::io(format!("install {}", dest.display()), e));
        }
        let _ = std::fs::remove_dir_all(&previous);
    } else {
        std::fs::rename(&staging, &dest)
            .map_err(|e| PluginError::io(format!("install {}", dest.display()), e))?;
    }

    let installed = InstalledPack {
        id,
        name: manifest.name,
        version: manifest.version,
        path: dest,
        files: copied.0,
        bytes: copied.1,
        skipped: plan.skipped,
        replaced,
    };
    info!(
        id = %installed.id,
        version = %installed.version,
        files = installed.files,
        bytes = installed.bytes,
        replaced = installed.replaced,
        skipped = installed.skipped.len(),
        "plug-in pack installed"
    );
    for entry in &installed.skipped {
        warn!(id = %installed.id, entry, "pack entry skipped — not allowed in a data-only pack");
    }
    Ok(installed)
}

/// Remove an installed pack. `Ok(false)` if there was nothing there.
pub fn uninstall(id: &str, data_dir: &Path) -> Result<bool, PluginError> {
    if !is_safe_id(id) {
        return Err(PluginError::UnsafeId(id.to_owned()));
    }
    let dir = data_dir.join(PLUGINS_DIR).join(id);
    if !dir.is_dir() {
        return Ok(false);
    }
    std::fs::remove_dir_all(&dir)
        .map_err(|e| PluginError::io(format!("remove {}", dir.display()), e))?;
    info!(%id, "plug-in pack removed");
    Ok(true)
}

/// Manifests of everything currently installed, sorted by id.
pub fn list_installed(data_dir: &Path) -> Vec<PluginManifest> {
    let plugins_dir = data_dir.join(PLUGINS_DIR);
    let Ok(entries) = std::fs::read_dir(&plugins_dir) else {
        return Vec::new();
    };
    let mut out: Vec<PluginManifest> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        // Staging directories are dot-prefixed and are not packs.
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| read_manifest(&e.path()).ok())
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Read and parse a pack's manifest.
pub fn read_manifest(dir: &Path) -> Result<PluginManifest, PluginError> {
    let path = dir.join(MANIFEST_NAME);
    let text = std::fs::read_to_string(&path).map_err(|_| PluginError::MissingManifest {
        dir: dir.to_path_buf(),
        manifest: MANIFEST_NAME.to_owned(),
    })?;
    toml::from_str(&text).map_err(|e| PluginError::BadManifest(e.to_string()))
}

/// Read the installer's view of a manifest: what kind of plug-in this
/// is, and — for an extension — everything it declares.
pub fn read_header(dir: &Path) -> Result<ManifestHeader, PluginError> {
    let path = dir.join(MANIFEST_NAME);
    let text = std::fs::read_to_string(&path).map_err(|_| PluginError::MissingManifest {
        dir: dir.to_path_buf(),
        manifest: MANIFEST_NAME.to_owned(),
    })?;
    toml::from_str(&text).map_err(|e| PluginError::BadManifest(e.to_string()))
}

/// A pack id becomes a directory name, so it may not contain anything
/// that escapes one or means something to a shell or a path parser.
pub fn is_safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id != "."
        && id != ".."
        && !id.starts_with('.')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// What a copy would move, decided before anything is written.
struct CopyPlan {
    /// Paths relative to the source root.
    files: Vec<PathBuf>,
    /// How many of `files` are actual content — layouts, wordlists,
    /// translations. A "pack" of manifest and README alone installs
    /// nothing and would leave a directory the loader silently ignores.
    content_files: usize,
    skipped: Vec<String>,
    bytes: u64,
}

/// Walk the source and decide what is installable.
///
/// One pass, no writes: the budget and the allow-list are enforced
/// here so that a refusal never leaves a partial install.
fn plan_copy(src: &Path, kind: PluginKind) -> Result<CopyPlan, PluginError> {
    let mut plan = CopyPlan {
        files: Vec::new(),
        content_files: 0,
        skipped: Vec::new(),
        bytes: 0,
    };

    for entry in read_dir_sorted(src)? {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let meta = std::fs::symlink_metadata(&path)
            .map_err(|e| PluginError::io(format!("stat {}", path.display()), e))?;

        if meta.file_type().is_symlink() {
            // Not followed, not copied: a symlink named
            // `layout-mappings` pointing at somewhere private would
            // otherwise become a copy of that.
            return Err(PluginError::UnsafePath(path));
        }
        if meta.is_dir() {
            match allowed_dirs(kind).iter().find(|(d, _)| *d == name) {
                Some((_, exts)) => collect_dir(src, &path, exts, &mut plan)?,
                None => plan.skipped.push(format!("{name}/")),
            }
            continue;
        }
        if ALLOWED_TOP_LEVEL.contains(&name.as_str()) {
            plan.bytes += meta.len();
            plan.files.push(PathBuf::from(&name));
        } else {
            plan.skipped.push(name);
        }
        check_budget(&plan)?;
    }

    check_budget(&plan)?;
    Ok(plan)
}

/// Which content directories this kind of plug-in may populate.
///
/// A pack gets the data directories and nothing else — that is what
/// makes "a pack cannot execute" a fact about the installer rather than
/// a hope about pack authors. An extension gets the same plus `bin/`.
fn allowed_dirs(kind: PluginKind) -> Vec<(&'static str, &'static [&'static str])> {
    let mut dirs = ALLOWED_CONTENT.to_vec();
    if kind == PluginKind::Extension {
        dirs.extend_from_slice(EXTENSION_CONTENT);
    }
    dirs
}

/// Collect the allowed files of one content directory.
fn collect_dir(
    src_root: &Path,
    dir: &Path,
    exts: &[&str],
    plan: &mut CopyPlan,
) -> Result<(), PluginError> {
    for entry in read_dir_sorted(dir)? {
        let path = entry.path();
        let meta = std::fs::symlink_metadata(&path)
            .map_err(|e| PluginError::io(format!("stat {}", path.display()), e))?;
        if meta.file_type().is_symlink() {
            return Err(PluginError::UnsafePath(path));
        }
        let rel = path
            .strip_prefix(src_root)
            .map_err(|_| PluginError::UnsafePath(path.clone()))?
            .to_path_buf();
        if !is_contained(&rel) {
            return Err(PluginError::UnsafePath(path));
        }
        // One level deep. Nesting buys nothing for a data pack and
        // would need its own traversal budget.
        if meta.is_dir() {
            plan.skipped.push(format!("{}/", rel.display()));
            continue;
        }
        // An empty extension list means "any file", which is `bin/`: a
        // Unix executable has no extension at all. Nothing in `bin/` is
        // ever loaded — it is spawned as a separate process, by the one
        // name the manifest declares.
        let ok = exts.is_empty()
            || path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| exts.iter().any(|a| a.eq_ignore_ascii_case(e)));
        if ok {
            plan.bytes += meta.len();
            plan.files.push(rel);
            plan.content_files += 1;
            check_budget(plan)?;
        } else {
            plan.skipped.push(rel.display().to_string());
        }
    }
    Ok(())
}

fn check_budget(plan: &CopyPlan) -> Result<(), PluginError> {
    if plan.bytes > MAX_PACK_BYTES {
        return Err(PluginError::TooLarge {
            actual: plan.bytes,
            limit: MAX_PACK_BYTES,
        });
    }
    if plan.files.len() > MAX_PACK_FILES {
        return Err(PluginError::TooManyFiles(MAX_PACK_FILES));
    }
    Ok(())
}

/// A relative path that stays inside its root: no `..`, no absolute
/// prefix, no root component.
fn is_contained(rel: &Path) -> bool {
    rel.components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

/// Perform the planned copy into `dest`. Returns `(files, bytes)`.
fn copy_plan(src: &Path, dest: &Path, plan: &CopyPlan) -> Result<(usize, u64), PluginError> {
    std::fs::create_dir_all(dest)
        .map_err(|e| PluginError::io(format!("create {}", dest.display()), e))?;
    let mut bytes = 0u64;
    for rel in &plan.files {
        // Re-checked at write time, not only at plan time: the two are
        // separated by I/O, and the check that matters is the one next
        // to the operation it protects.
        if !is_contained(rel) {
            return Err(PluginError::UnsafePath(rel.clone()));
        }
        let to = dest.join(rel);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| PluginError::io(format!("create {}", parent.display()), e))?;
        }
        let n = std::fs::copy(src.join(rel), &to)
            .map_err(|e| PluginError::io(format!("copy {}", rel.display()), e))?;
        bytes += n;
    }
    Ok((plan.files.len(), bytes))
}

fn read_dir_sorted(dir: &Path) -> Result<Vec<std::fs::DirEntry>, PluginError> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| PluginError::io(format!("read {}", dir.display()), e))?
        .flatten()
        .collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    Ok(entries)
}
