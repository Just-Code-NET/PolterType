//! `cargo xtask` — internal helper commands.
//!
//! ## `cargo xtask wordlists fetch`
//!
//! Re-downloads and re-processes the embedded language dictionaries.
//! Sources are documented in `data/wordlists/CREDITS.md`.

#![allow(clippy::unwrap_used, clippy::expect_used)] // build/dev tool

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

const EN_URL: &str = "https://raw.githubusercontent.com/dwyl/english-words/master/words_alpha.txt";
const UK_URL: &str =
    "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/uk_UA/uk_UA.dic";
const UK_README_URL: &str =
    "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/uk_UA/README_uk_UA.txt";

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    match (args.next().as_deref(), args.next().as_deref()) {
        (Some("help") | None, _) => {
            print_help();
            Ok(())
        }
        (Some("wordlists"), Some("fetch")) => fetch_wordlists(),
        (Some(other), _) => bail!("unknown xtask command: {other} (try `cargo xtask help`)"),
    }
}

fn print_help() {
    println!("xtask commands:");
    println!("  help                  Show this list.");
    println!("  wordlists fetch       Re-download and re-process the embedded dictionaries.");
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

    println!("\nDone. Review with `git diff data/wordlists/` and commit.");
    Ok(())
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
