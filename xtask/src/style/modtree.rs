//! Does the module tree still hold together after files were moved?
//!
//! The one check a single-platform `cargo check` cannot do: a file
//! reachable only from `#[cfg(windows)] mod …` is invisible here, so a
//! `mod` left pointing at nothing — or a file left behind that no
//! `mod` declares — compiles clean on this machine and fails on
//! someone else's. `cfg` is deliberately not evaluated: every `mod`
//! counts, whichever OS compiles it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::enums::{Kind, Rule};
use super::types::{FileScan, Finding};

/// A file that starts a module tree of its own rather than being
/// declared by some other file.
fn is_crate_root(rel: &Path) -> bool {
    let name = rel.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if matches!(name, "lib.rs" | "main.rs" | "build.rs") {
        return true;
    }
    // `crates/<name>/tests/*.rs` — one integration-test crate each.
    rel.components()
        .filter_map(|c| c.as_os_str().to_str())
        .any(|c| c == "tests")
        && rel.parent().and_then(Path::file_name) == Some(std::ffi::OsStr::new("tests"))
}

/// Where the submodules of `rel` live.
fn module_dir(rel: &Path) -> PathBuf {
    let parent = rel.parent().unwrap_or(Path::new("")).to_path_buf();
    match rel.file_name().and_then(|n| n.to_str()) {
        Some("lib.rs") | Some("main.rs") | Some("mod.rs") | Some("build.rs") => parent,
        _ => parent.join(rel.file_stem().unwrap_or_default()),
    }
}

pub(crate) fn check(files: &[(PathBuf, FileScan)]) -> Vec<Finding> {
    let known: BTreeSet<&Path> = files.iter().map(|(p, _)| p.as_path()).collect();
    let mut reached: BTreeSet<PathBuf> = BTreeSet::new();
    let mut out = Vec::new();

    for (rel, scan) in files {
        if is_crate_root(rel) {
            reached.insert(rel.clone());
        }
        let dir = module_dir(rel);
        for item in &scan.items {
            // `#[path]` is rejected by its own rule; resolving it here
            // would report the same module twice.
            if item.kind != Kind::Mod || item.has_body || item.path_attr {
                continue;
            }
            let flat = dir.join(format!("{}.rs", item.name));
            let nested = dir.join(&item.name).join("mod.rs");
            if known.contains(flat.as_path()) {
                reached.insert(flat);
            } else if known.contains(nested.as_path()) {
                reached.insert(nested);
            } else {
                out.push(Finding {
                    file: rel.clone(),
                    line: item.line,
                    rule: Rule::ModTree,
                    message: format!(
                        "`mod {}` has no file — expected `{}` or `{}`",
                        item.name,
                        flat.display(),
                        nested.display()
                    ),
                });
            }
        }
    }

    for (rel, _) in files {
        if !reached.contains(rel) {
            out.push(Finding {
                file: rel.clone(),
                line: 1,
                rule: Rule::ModTree,
                message: "no `mod` declares this file — it is compiled by nobody".to_owned(),
            });
        }
    }

    out
}
