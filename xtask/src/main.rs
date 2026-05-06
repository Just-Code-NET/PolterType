//! `cargo xtask` — internal helper commands.
//!
//! ## `cargo xtask wordlists fetch`
//!
//! Re-downloads and re-processes the embedded language dictionaries.
//! Sources are documented in `data/wordlists/CREDITS.md`.
//!
//! ## `cargo xtask hooks install` / `cargo xtask hooks uninstall`
//!
//! Wires (or unwires) the versioned git hooks under `.githooks/`.
//! See `.githooks/README.md` for what each hook enforces.

#![allow(clippy::unwrap_used, clippy::expect_used)] // build/dev tool

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

const EN_URL: &str = "https://raw.githubusercontent.com/dwyl/english-words/master/words_alpha.txt";
const UK_URL: &str =
    "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/uk_UA/uk_UA.dic";
const UK_README_URL: &str =
    "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/uk_UA/README_uk_UA.txt";

// Hunspell-derived sources for the additional bundled languages.
// All four share the same `<word>/<affix-flags>` row format used by
// uk_UA, so `process_hunspell_dic` handles them uniformly.
const RU_URL: &str =
    "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/ru_RU/ru_RU.dic";
const DE_URL: &str =
    "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/de/de_DE_frami.dic";
const ES_URL: &str =
    "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/es/es_ES.dic";
const FR_URL: &str =
    "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/fr_FR/fr.dic";

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    match (args.next().as_deref(), args.next().as_deref()) {
        (Some("help") | None, _) => {
            print_help();
            Ok(())
        }
        (Some("wordlists"), Some("fetch")) => fetch_wordlists(),
        (Some("hooks"), Some("install")) => install_hooks(),
        (Some("hooks"), Some("uninstall")) => uninstall_hooks(),
        (Some(other), _) => bail!("unknown xtask command: {other} (try `cargo xtask help`)"),
    }
}

fn print_help() {
    println!("xtask commands:");
    println!("  help                  Show this list.");
    println!("  wordlists fetch       Re-download and re-process the embedded dictionaries.");
    println!("  hooks install         Wire `.githooks/` into this clone (sets core.hooksPath).");
    println!("  hooks uninstall       Unset core.hooksPath (revert to default `.git/hooks/`).");
}

fn fetch_wordlists() -> Result<()> {
    let repo_root = repo_root()?;
    let wl_dir = repo_root.join("data").join("wordlists");
    let src_dir = wl_dir.join("sources");
    fs::create_dir_all(&src_dir).with_context(|| format!("create {}", src_dir.display()))?;

    println!("Downloading {EN_URL}");
    let en_raw = http_get(EN_URL)?;
    let en_src_path = src_dir.join("words_alpha.txt");
    fs::write(&en_src_path, &en_raw).with_context(|| format!("write {}", en_src_path.display()))?;
    println!("  saved {} ({} bytes)", en_src_path.display(), en_raw.len());

    println!("Downloading {UK_URL}");
    let uk_raw = http_get(UK_URL)?;
    let uk_src_path = src_dir.join("uk_UA.dic");
    fs::write(&uk_src_path, &uk_raw).with_context(|| format!("write {}", uk_src_path.display()))?;
    println!("  saved {} ({} bytes)", uk_src_path.display(), uk_raw.len());

    println!("Downloading {UK_README_URL}");
    let uk_readme = http_get(UK_README_URL)?;
    let uk_readme_path = src_dir.join("uk_UA-README.txt");
    fs::write(&uk_readme_path, &uk_readme)
        .with_context(|| format!("write {}", uk_readme_path.display()))?;

    process_en(&en_src_path, &wl_dir.join("en_us.txt"))?;
    process_uk(&uk_src_path, &wl_dir.join("uk_ua.txt"))?;

    // Newer bundled languages: same Hunspell `.dic` format, processed
    // generically. Each one is independent — a transient 404 / 5xx on
    // one source shouldn't take the whole script down, so we log and
    // continue.
    fetch_hunspell(&src_dir, &wl_dir, "ru_RU.dic", RU_URL, "ru_ru.txt");
    fetch_hunspell(&src_dir, &wl_dir, "de_DE_frami.dic", DE_URL, "de_de.txt");
    fetch_hunspell(&src_dir, &wl_dir, "es_ES.dic", ES_URL, "es_es.txt");
    fetch_hunspell(&src_dir, &wl_dir, "fr.dic", FR_URL, "fr_fr.txt");

    println!("\nDone. Review with `git diff data/wordlists/` and commit.");
    Ok(())
}

/// Download one Hunspell-format `.dic`, drop it under `sources/`,
/// then process it through `process_hunspell_dic` into `<wl_dir>/<output>`.
/// Errors are surfaced on stderr but don't abort the rest of the
/// fetch run — partial progress is better than none for a multi-source
/// script.
fn fetch_hunspell(src_dir: &Path, wl_dir: &Path, src_name: &str, url: &str, output: &str) {
    println!("Downloading {url}");
    match http_get(url) {
        Ok(raw) => {
            let src_path = src_dir.join(src_name);
            if let Err(e) = fs::write(&src_path, &raw) {
                eprintln!("  write {} failed: {e}", src_path.display());
                return;
            }
            println!("  saved {} ({} bytes)", src_path.display(), raw.len());
            let out = wl_dir.join(output);
            if let Err(e) = process_hunspell_dic(&src_path, &out) {
                eprintln!("  process {} failed: {e}", src_path.display());
            }
        }
        Err(e) => eprintln!("  download {url} failed: {e}"),
    }
}

fn process_en(input: &Path, output: &Path) -> Result<()> {
    let raw = fs::read_to_string(input).with_context(|| format!("read {}", input.display()))?;
    let mut words: BTreeSet<String> = BTreeSet::new();
    for line in raw.lines() {
        let w = line.trim().to_ascii_lowercase();
        if w.is_empty() {
            continue;
        }
        // Keep only pure-letter words.
        if w.chars().all(|c| c.is_ascii_lowercase()) {
            words.insert(w);
        }
    }
    write_sorted(output, &words)?;
    println!("  en_us.txt: {} words", words.len());
    Ok(())
}

/// Generic Hunspell `.dic` processor: same row shape as uk_UA — first
/// line is the entry count, then `<word>/<affix-flags>` per line. Used
/// for ru_RU, de_DE, es_ES, fr_FR. We strip affix flags, lowercase,
/// and keep entries that look like real words (letters + apostrophe +
/// hyphen). The result is sorted + deduped and written to `output`.
///
/// Why this is one function instead of one per language: the cleanup
/// rules are identical across all four (no language ships
/// language-specific noise we'd want to special-case here), and the
/// FST builder doesn't care about quality differences past "is it
/// alphabetic". If we ever need per-language post-processing, fork
/// per-language wrappers like `process_uk` did.
fn process_hunspell_dic(input: &Path, output: &Path) -> Result<()> {
    let raw = fs::read_to_string(input).with_context(|| format!("read {}", input.display()))?;
    let mut words: BTreeSet<String> = BTreeSet::new();
    let mut iter = raw.lines();
    let _count = iter.next(); // first Hunspell line is the entry count
    for line in iter {
        let line = line.trim();
        if line.is_empty() || line.starts_with('+') || line.starts_with('#') {
            continue;
        }
        let stem = line.split('/').next().unwrap_or(line).trim();
        if stem.is_empty() {
            continue;
        }
        let lower: String = stem.chars().flat_map(char::to_lowercase).collect();
        let acceptable = lower
            .chars()
            .all(|c| c.is_alphabetic() || matches!(c, '\'' | '-' | 'ʼ' | '\u{2019}'));
        let has_letter = lower.chars().any(|c| c.is_alphabetic());
        if acceptable && has_letter {
            words.insert(lower);
        }
    }
    write_sorted(output, &words)?;
    println!(
        "  {}: {} words",
        output.file_name().and_then(|s| s.to_str()).unwrap_or("?"),
        words.len()
    );
    Ok(())
}

fn process_uk(input: &Path, output: &Path) -> Result<()> {
    let raw = fs::read_to_string(input).with_context(|| format!("read {}", input.display()))?;
    let mut words: BTreeSet<String> = BTreeSet::new();
    let mut iter = raw.lines();
    let _count = iter.next(); // first Hunspell line is the entry count
    for line in iter {
        let line = line.trim();
        if line.is_empty() || line.starts_with('+') {
            continue;
        }
        // Strip Hunspell `/affixflags` suffix.
        let stem = line.split('/').next().unwrap_or(line).trim();
        if stem.is_empty() {
            continue;
        }
        let lower: String = stem.chars().flat_map(char::to_lowercase).collect();
        // Keep only entries made of letters / apostrophe / hyphen,
        // and require at least one alphabetic character.
        let acceptable = lower
            .chars()
            .all(|c| c.is_alphabetic() || matches!(c, '\'' | '-' | 'ʼ' | '\u{2019}'));
        let has_letter = lower.chars().any(|c| c.is_alphabetic());
        if acceptable && has_letter {
            words.insert(lower);
        }
    }
    write_sorted(output, &words)?;
    println!("  uk_ua.txt: {} words", words.len());
    Ok(())
}

fn write_sorted(path: &Path, words: &BTreeSet<String>) -> Result<()> {
    let mut w = BufWriter::new(File::create(path)?);
    for word in words {
        writeln!(w, "{word}")?;
    }
    Ok(())
}

fn http_get(url: &str) -> Result<Vec<u8>> {
    let resp = ureq::get(url)
        .timeout(std::time::Duration::from_secs(120))
        .call()?;
    let mut buf = Vec::new();
    resp.into_reader().read_to_end(&mut buf)?;
    Ok(buf)
}

fn repo_root() -> Result<PathBuf> {
    // CARGO_MANIFEST_DIR for xtask = <root>/xtask; go up one.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .map(Path::to_path_buf)
        .context("xtask CARGO_MANIFEST_DIR has no parent")
}

// ─── `hooks install` / `hooks uninstall` ──────────────────────────────

/// Wire the versioned `.githooks/` directory into the local clone by
/// setting `core.hooksPath`. This is the entire install — Git itself
/// runs every executable in that directory whose name matches a hook
/// stage, so we don't need to touch `.git/hooks/` at all (and stay
/// out of its way for users who already keep something there).
///
/// We also re-`chmod +x` the scripts on POSIX after the config write,
/// in case someone fetched the repo via a tool that didn't preserve
/// the executable bit (rare but happens with raw zip downloads).
fn install_hooks() -> Result<()> {
    let root = repo_root()?;
    let hooks_dir = root.join(".githooks");
    if !hooks_dir.exists() {
        bail!(
            "expected hooks directory at {} — refusing to set core.hooksPath to a missing path",
            hooks_dir.display()
        );
    }

    // Path stored in `git config` is interpreted relative to the
    // working tree root, so `.githooks` (no leading slash) is correct
    // and portable across platforms.
    let status = Command::new("git")
        .args(["config", "core.hooksPath", ".githooks"])
        .current_dir(&root)
        .status()
        .context("invoke `git config core.hooksPath`")?;
    if !status.success() {
        bail!("`git config core.hooksPath .githooks` failed (status: {status})");
    }

    #[cfg(unix)]
    chmod_executable(&hooks_dir)?;

    println!("kb-switcher hooks installed:");
    println!("  pre-commit  →  cargo fmt --all -- --check");
    println!("  pre-push    →  cargo build --workspace --all-targets");
    println!();
    println!("Bypass any single run with `git commit --no-verify` / `git push --no-verify`.");
    Ok(())
}

/// Inverse of `install_hooks`: drop the `core.hooksPath` config so
/// Git falls back to its default (`.git/hooks/`, empty in fresh
/// clones). `--unset` is a no-op if the config wasn't set, but git
/// returns exit-code 5 in that case — we suppress it to keep the
/// command idempotent ("uninstall what isn't installed → success").
fn uninstall_hooks() -> Result<()> {
    let root = repo_root()?;
    let output = Command::new("git")
        .args(["config", "--unset", "core.hooksPath"])
        .current_dir(&root)
        .output()
        .context("invoke `git config --unset core.hooksPath`")?;
    // git returns 5 if the key wasn't set — treat as success.
    let code = output.status.code().unwrap_or_default();
    if !output.status.success() && code != 5 {
        bail!(
            "`git config --unset core.hooksPath` failed (status: {}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    println!("kb-switcher hooks uninstalled — Git will use the default `.git/hooks/` from now on.");
    Ok(())
}

/// Best-effort `chmod +x` for every regular file inside `dir`.
/// Avoids dragging the `nix` crate in for a one-call use; we just
/// shell out to `chmod` which is on every Unix that runs Git anyway.
/// On non-Unix this whole function is `cfg`-skipped — Git for Windows
/// runs hooks via its bundled `sh.exe` regardless of file mode.
#[cfg(unix)]
fn chmod_executable(dir: &Path) -> Result<()> {
    let entries = fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        // Skip the README — it's documentation, not a hook.
        if path
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|n| n.starts_with("README"))
        {
            continue;
        }
        let _ = Command::new("chmod").arg("+x").arg(&path).status();
    }
    Ok(())
}
