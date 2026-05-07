//! `cargo xtask version` — bump or set the workspace version.
//!
//! Edits the three files that need to stay in lock-step on every
//! release (`Cargo.toml`, `CHANGELOG.md`, `Cargo.lock`) so the
//! release-cutter doesn't have to remember the order or the exact
//! patterns. See `docs/RELEASING.md` for the full release flow this
//! command fits into.
//!
//! ## Surface
//!
//! ```text
//! cargo xtask version                    # print current
//! cargo xtask version bump               # auto-bump
//! cargo xtask version set <NEW>          # exact set
//! cargo xtask version <subcommand> --dry-run
//! ```
//!
//! "Auto-bump" rule: if the current version has a pre-release suffix
//! shaped like `-<word>.<N>` (e.g. `-beta.5`), increment the
//! trailing counter (`-beta.6`). Otherwise, increment the patch
//! component (`1.2.3` → `1.2.4`). This matches the project's actual
//! lifecycle — every kb-switcher release so far has been a bump of
//! the `alpha.<N>` / `beta.<N>` counter, with the major/minor/patch
//! transitions ("alpha → beta", "beta → rc", "drop suffix on 1.0")
//! happening rarely enough that they're worth doing explicitly with
//! `set` rather than guessing in the script.
//!
//! ## What the bump touches
//!
//! 1. `Cargo.toml` — `[workspace.package].version = "..."`
//! 2. `CHANGELOG.md` — top-level `## [Unreleased] — <ver>` heading
//!    if present (skipped with a warning if it isn't, so the file
//!    isn't required to exist or follow a particular shape).
//! 3. `Cargo.lock` — refreshed via `cargo check --workspace`.
//!
//! ## What it deliberately does NOT do
//!
//! * Commit, tag, or push. Release commits should be reviewed
//!   manually — that's the moment to catch a wrong bump or a
//!   missing changelog entry, not after the tag has hit CI. The
//!   doc at `docs/RELEASING.md` walks through the commit + tag
//!   step explicitly.
//! * Talk to the network. Pre-release validation (does this version
//!   already exist as a tag?) is the user's job — `git tag -l` is
//!   one line in the release checklist.
//! * Pull in `semver` / `regex` deps. The version shapes we ship
//!   are a small subset; the hand-rolled parser below covers them
//!   in ~30 lines.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

/// Entry point — dispatches `version [bump|set <V>] [--dry-run]`.
pub fn run(rest: &[String]) -> Result<()> {
    // Strip an optional trailing `--dry-run` flag. We accept it
    // anywhere after the subcommand so users don't have to remember
    // exact ordering.
    let dry_run = rest.iter().any(|a| a == "--dry-run");
    let positional: Vec<&str> = rest
        .iter()
        .filter(|a| *a != "--dry-run")
        .map(String::as_str)
        .collect();

    match positional.as_slice() {
        [] => print_current(),
        ["bump"] => apply_change(Change::Bump, dry_run),
        ["set", new] => apply_change(Change::Set((*new).to_owned()), dry_run),
        ["bump", _, ..] | ["set", _, _, ..] => {
            bail!("too many arguments — see `cargo xtask help`");
        }
        ["set"] => bail!("`cargo xtask version set` needs an argument, e.g. `set 0.1.0-beta.7`"),
        [other, ..] => bail!("unknown version subcommand `{other}` (expected `bump` or `set`)"),
    }
}

enum Change {
    Bump,
    Set(String),
}

fn print_current() -> Result<()> {
    let path = workspace_cargo_toml()?;
    let body = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let cur =
        read_version(&body).with_context(|| format!("locate version in {}", path.display()))?;
    println!("{cur}");
    Ok(())
}

fn apply_change(change: Change, dry_run: bool) -> Result<()> {
    let cargo_toml = workspace_cargo_toml()?;
    let body = fs::read_to_string(&cargo_toml)
        .with_context(|| format!("read {}", cargo_toml.display()))?;
    let current = read_version(&body)
        .with_context(|| format!("locate version in {}", cargo_toml.display()))?;

    let next = match &change {
        Change::Bump => bump(&current)?,
        Change::Set(new) => {
            // Light validation so a typo doesn't leak into the file.
            // The parser accepts everything we'd ever want to set;
            // a string it rejects is almost certainly a mistake.
            parse(new).with_context(|| format!("`{new}` is not a recognised version shape"))?;
            new.clone()
        }
    };

    if next == current {
        println!("version already {current} — nothing to do");
        return Ok(());
    }

    println!(
        "{current} → {next}{}",
        if dry_run { "  (dry-run)" } else { "" }
    );

    if dry_run {
        return Ok(());
    }

    // Step 1: rewrite Cargo.toml — single line, surgical replace
    // anchored on the leading `version       = ` so we don't
    // accidentally replace a version string inside a doc comment.
    let new_body = replace_version_line(&body, &current, &next)?;
    fs::write(&cargo_toml, new_body).with_context(|| format!("write {}", cargo_toml.display()))?;
    println!("  ✓ Cargo.toml");

    // Step 2: update CHANGELOG.md heading if present. Missing /
    // mismatched is a warning, not an error — keeps the script
    // useful in a fresh checkout that hasn't grown a CHANGELOG
    // yet, and avoids forcing every consumer of the script to
    // adopt our exact heading shape.
    let changelog = workspace_root()?.join("CHANGELOG.md");
    match update_changelog(&changelog, &current, &next) {
        Ok(true) => println!("  ✓ CHANGELOG.md"),
        Ok(false) => println!(
            "  · CHANGELOG.md heading not found — skipping (add `## [Unreleased] — {current}` to enable)"
        ),
        Err(e) => println!("  · CHANGELOG.md update failed: {e} — skipping"),
    }

    // Step 3: refresh Cargo.lock by running `cargo check`. Any
    // alternative (manually rewriting the lock, calling cargo
    // metadata) would either drift or pull in cargo as a dep —
    // shelling out is honest about what we're doing.
    println!("  · refreshing Cargo.lock via `cargo check --workspace` ...");
    let status = Command::new(cargo_bin())
        .args(["check", "--workspace", "--quiet"])
        .current_dir(workspace_root()?)
        .status()
        .context("spawn `cargo check`")?;
    if !status.success() {
        bail!(
            "`cargo check --workspace` failed — Cargo.lock may be out of sync with the new version"
        );
    }
    println!("  ✓ Cargo.lock");

    println!();
    println!("Next steps (see docs/RELEASING.md for the full checklist):");
    println!("  git diff Cargo.toml Cargo.lock CHANGELOG.md");
    println!("  git add Cargo.toml Cargo.lock CHANGELOG.md");
    println!("  git commit -m \"release: v{next}\"");
    println!("  git tag v{next}");
    println!("  git push origin HEAD --tags");

    Ok(())
}

// ─── Pure version-string helpers (heavily unit-tested) ──────────────

/// Versioned identifier as we use them: `MAJOR.MINOR.PATCH` plus an
/// optional pre-release suffix `-<word>.<counter>`. This is a
/// **subset** of full SemVer — we don't accept multiple suffix
/// components (`-alpha.1.beta`), arbitrary build metadata
/// (`+build.42`), or non-numeric counters (`-rc-final`). That's
/// fine: every kb-switcher release we've ever cut fits the subset,
/// and rejecting weirder shapes catches typos that a permissive
/// parser would silently accept.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Version {
    major: u64,
    minor: u64,
    patch: u64,
    pre: Option<PreRelease>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PreRelease {
    /// `alpha`, `beta`, `rc`, …
    word: String,
    counter: u64,
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(p) = &self.pre {
            write!(f, "-{}.{}", p.word, p.counter)?;
        }
        Ok(())
    }
}

fn parse(s: &str) -> Result<Version> {
    let (core, pre) = match s.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (s, None),
    };
    let mut parts = core.split('.');
    let major: u64 = parts
        .next()
        .context("missing MAJOR")?
        .parse()
        .with_context(|| format!("MAJOR is not a number in `{s}`"))?;
    let minor: u64 = parts
        .next()
        .with_context(|| format!("missing MINOR in `{s}`"))?
        .parse()
        .with_context(|| format!("MINOR is not a number in `{s}`"))?;
    let patch: u64 = parts
        .next()
        .with_context(|| format!("missing PATCH in `{s}`"))?
        .parse()
        .with_context(|| format!("PATCH is not a number in `{s}`"))?;
    if parts.next().is_some() {
        bail!("`{s}` has a fourth dotted segment — only MAJOR.MINOR.PATCH is supported");
    }
    let pre = match pre {
        None => None,
        Some(p) => {
            let (word, counter) = p
                .split_once('.')
                .with_context(|| format!("pre-release `{p}` must be `<word>.<counter>`"))?;
            if word.is_empty() || !word.chars().all(|c| c.is_ascii_alphabetic()) {
                bail!("pre-release word in `{s}` must be ASCII letters (e.g. `alpha`, `beta`)");
            }
            let counter: u64 = counter
                .parse()
                .with_context(|| format!("pre-release counter in `{s}` is not a number"))?;
            Some(PreRelease {
                word: word.to_owned(),
                counter,
            })
        }
    };
    Ok(Version {
        major,
        minor,
        patch,
        pre,
    })
}

fn bump(s: &str) -> Result<String> {
    let mut v = parse(s)?;
    if let Some(p) = &mut v.pre {
        p.counter += 1;
    } else {
        v.patch += 1;
    }
    Ok(v.to_string())
}

fn read_version(cargo_toml_body: &str) -> Result<String> {
    // We deliberately don't pull in toml-edit just for one read —
    // the workspace.package.version line has a fixed shape that's
    // been the same since Phase 0 of the project.
    for line in cargo_toml_body.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("version") {
            // Match e.g. `version       = "0.1.0-beta.6"` or
            // `version="..."`. Anchored on the start of trimmed
            // text so a stray `version` substring inside a comment
            // doesn't match.
            let after_eq = rest.trim_start().strip_prefix('=').map(str::trim_start);
            if let Some(after_eq) = after_eq {
                if let Some(quoted) = after_eq.strip_prefix('"') {
                    if let Some(end) = quoted.find('"') {
                        return Ok(quoted[..end].to_owned());
                    }
                }
            }
        }
    }
    bail!("could not find a `version = \"...\"` line in Cargo.toml")
}

fn replace_version_line(body: &str, current: &str, next: &str) -> Result<String> {
    // Replace the FIRST occurrence of `version = "<current>"`.
    // Doing a global string replace would also rewrite e.g. a
    // `kb-types = { version = "0.1.0-beta.6" }` inside a dep entry
    // — we only want the workspace.package one, which is the first
    // version line in our Cargo.toml shape.
    let needle = format!("version       = \"{current}\"");
    if let Some(found) = body.find(&needle) {
        let mut out = String::with_capacity(body.len());
        out.push_str(&body[..found]);
        out.push_str(&format!("version       = \"{next}\""));
        out.push_str(&body[found + needle.len()..]);
        return Ok(out);
    }
    // Fallback for Cargo.toml shapes that don't pad the equals
    // (e.g. someone reformatted the file). Less surgical, but only
    // matches the FIRST `version = "<current>"` so we still don't
    // touch dep version pins later in the file.
    let alt_needle = format!("version = \"{current}\"");
    if let Some(found) = body.find(&alt_needle) {
        let mut out = String::with_capacity(body.len());
        out.push_str(&body[..found]);
        out.push_str(&format!("version = \"{next}\""));
        out.push_str(&body[found + alt_needle.len()..]);
        return Ok(out);
    }
    bail!("could not find `version = \"{current}\"` line to replace in Cargo.toml")
}

fn update_changelog(path: &Path, current: &str, next: &str) -> Result<bool> {
    let Ok(body) = fs::read_to_string(path) else {
        return Ok(false);
    };
    // We only auto-update `## [Unreleased] — <ver>` headings — that's
    // the convention this repo follows. A user with a different
    // CHANGELOG shape gets the warning message and can edit by hand.
    let needle = format!("## [Unreleased] — {current}");
    let Some(found) = body.find(&needle) else {
        return Ok(false);
    };
    let mut out = String::with_capacity(body.len());
    out.push_str(&body[..found]);
    out.push_str(&format!("## [Unreleased] — {next}"));
    out.push_str(&body[found + needle.len()..]);
    fs::write(path, out).with_context(|| format!("write {}", path.display()))?;
    Ok(true)
}

// ─── Path / cargo helpers ──────────────────────────────────────────

fn workspace_root() -> Result<PathBuf> {
    // The script is always invoked via `cargo xtask`, which sets
    // `CARGO_MANIFEST_DIR` to the xtask crate's directory. The
    // workspace root is one level up.
    let manifest = std::env::var("CARGO_MANIFEST_DIR")
        .context("CARGO_MANIFEST_DIR not set — run via `cargo xtask`, not the binary directly")?;
    Ok(PathBuf::from(manifest)
        .parent()
        .context("CARGO_MANIFEST_DIR has no parent")?
        .to_owned())
}

fn workspace_cargo_toml() -> Result<PathBuf> {
    Ok(workspace_root()?.join("Cargo.toml"))
}

fn cargo_bin() -> String {
    // Honour `CARGO` if cargo set it (it always does when run via
    // `cargo xtask`); fall back to the bare command for the rare
    // case of running the xtask binary directly.
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The parser must accept every shape of version string we've
    /// ever shipped (and a couple of likely-future shapes) — and
    /// reject the kinds of typos a hand-edited config could have.
    #[test]
    fn parse_accepts_known_shapes() {
        for ok in [
            "0.1.0",
            "0.1.0-alpha.0",
            "0.1.0-alpha.5",
            "0.1.0-beta.6",
            "0.1.0-rc.1",
            "1.2.3",
            "10.20.30",
        ] {
            assert!(parse(ok).is_ok(), "`{ok}` should parse");
        }
    }

    #[test]
    fn parse_rejects_garbage() {
        for bad in [
            "",
            "1",
            "1.2",
            "1.2.3.4",          // four segments
            "1.2.x",            // non-numeric patch
            "1.2.3-",           // empty pre-release
            "1.2.3-beta",       // pre-release missing counter
            "1.2.3-beta.x",     // non-numeric counter
            "1.2.3-rc.1.extra", // multi-segment pre-release
            "1.2.3-be ta.1",    // whitespace in word
            "1.2.3-rc1.0",      // word contains digits (we forbid this — keep it simple)
        ] {
            assert!(parse(bad).is_err(), "`{bad}` should NOT parse");
        }
    }

    /// Display round-trip: parse, format, parse again — must yield
    /// the same struct. Catches subtle drift in the formatter.
    #[test]
    fn parse_format_round_trip() {
        for v in [
            "0.1.0",
            "0.1.0-alpha.0",
            "0.1.0-beta.6",
            "1.2.3-rc.99",
            "10.20.30",
        ] {
            let parsed = parse(v).expect("parse");
            assert_eq!(parsed.to_string(), v);
        }
    }

    /// `bump` rules pinned in tests so we can never accidentally
    /// change the auto-bump semantics without noticing.
    #[test]
    fn bump_pre_release_increments_counter() {
        assert_eq!(bump("0.1.0-beta.5").unwrap(), "0.1.0-beta.6");
        assert_eq!(bump("0.1.0-alpha.0").unwrap(), "0.1.0-alpha.1");
        assert_eq!(bump("1.2.3-rc.99").unwrap(), "1.2.3-rc.100");
    }

    #[test]
    fn bump_stable_increments_patch() {
        assert_eq!(bump("0.1.0").unwrap(), "0.1.1");
        assert_eq!(bump("1.2.3").unwrap(), "1.2.4");
        assert_eq!(bump("0.1.99").unwrap(), "0.1.100");
    }

    /// Cargo.toml line surgery — must replace the workspace
    /// version while leaving dep pins (`version = "1.0"`) alone.
    /// This is the regression we care most about: a global string
    /// replace would silently demote every internal crate version
    /// line in a workspace.dependencies block.
    #[test]
    fn replace_version_line_only_touches_first_occurrence() {
        let body = r#"
[workspace.package]
version       = "0.1.0-beta.6"
edition       = "2024"

[workspace.dependencies]
some-crate = { version = "0.1.0-beta.6" }
"#;
        let out = replace_version_line(body, "0.1.0-beta.6", "0.1.0-beta.7").unwrap();
        // First occurrence (the workspace.package one) is bumped.
        assert!(out.contains("version       = \"0.1.0-beta.7\""));
        // Second occurrence (the dep entry) is NOT touched.
        assert!(out.contains("some-crate = { version = \"0.1.0-beta.6\" }"));
    }

    /// `read_version` must locate the workspace version even when
    /// other `version` keys appear in the file (in deps, in
    /// comments, etc.). We don't care about the exact ordering of
    /// fields, only that the first `version = "..."` line wins —
    /// which is also what `replace_version_line` relies on.
    #[test]
    fn read_version_finds_workspace_package_version() {
        let body = r#"
# version of the schema, unrelated.
[workspace.package]
version       = "0.1.0-beta.6"
edition       = "2024"
"#;
        assert_eq!(read_version(body).unwrap(), "0.1.0-beta.6");
    }

    /// `read_version` produces a clear error when the file doesn't
    /// have the expected shape — the script is more useful when it
    /// fails loudly than when it pretends to succeed.
    #[test]
    fn read_version_errors_on_missing_field() {
        let body = "[package]\nname = \"nope\"\n";
        assert!(read_version(body).is_err());
    }
}
