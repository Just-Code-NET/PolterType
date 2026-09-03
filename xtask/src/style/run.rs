//! The `cargo xtask style` subcommand: walk, check, report.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use super::modtree;
use super::rules::check_file;
use super::scan::scan;
use super::types::{FileScan, Finding};
use crate::style::consts::ROOTS;

/// Check the repository against `CONTRIBUTING.md` § File organization
/// and § Style & guarantees.
///
/// An optional path prefix narrows the report to one crate or
/// directory; the module-tree check still runs over everything, since
/// a `mod` and its file need not be in the same crate subtree.
pub(crate) fn run(args: &[String]) -> Result<()> {
    let root = crate::paths::repo_root()?;
    let filter = args.iter().find(|a| !a.starts_with('-')).cloned();

    let mut files: Vec<(PathBuf, FileScan)> = Vec::new();
    for dir in ROOTS {
        collect(&root.join(dir), &root, &mut files)?;
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut findings: Vec<Finding> = Vec::new();
    for (rel, file) in &files {
        findings.extend(check_file(rel, file));
    }
    findings.extend(modtree::check(&files));

    if let Some(prefix) = &filter {
        findings.retain(|f| f.file.starts_with(prefix));
    }
    findings.sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));

    for f in &findings {
        println!(
            "{}:{}: [{}] {}",
            f.file.display(),
            f.line,
            f.rule,
            f.message
        );
    }

    if findings.is_empty() {
        println!("style: {} files, no violations", files.len());
        return Ok(());
    }
    bail!(
        "style: {} violations in {} files (see CONTRIBUTING.md § File organization)",
        findings.len(),
        findings
            .iter()
            .map(|f| &f.file)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    )
}

fn collect(dir: &Path, root: &Path, out: &mut Vec<(PathBuf, FileScan)>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_dir() {
            if name != "target" {
                collect(&path, root, out)?;
            }
        } else if name.ends_with(".rs") {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
            out.push((rel, scan(&text)));
        }
    }
    Ok(())
}
