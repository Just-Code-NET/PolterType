//! Wordlist pipeline: download, Hunspell-expand, write sorted.

use crate::*;
use anyhow::{Context, Result, bail};
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::Path;

pub(crate) fn fetch_wordlists() -> Result<()> {
    let repo_root = repo_root()?;
    let wl_dir = repo_root.join("data").join("wordlists");
    let src_dir = wl_dir.join("sources");
    fs::create_dir_all(&src_dir).with_context(|| format!("create {}", src_dir.display()))?;

    // English: plain word list, no Hunspell rules to apply.
    println!("Downloading {EN_URL}");
    let en_raw = http_get(EN_URL)?;
    let en_src_path = src_dir.join("words_alpha.txt");
    fs::write(&en_src_path, &en_raw).with_context(|| format!("write {}", en_src_path.display()))?;
    println!("  saved {} ({} bytes)", en_src_path.display(), en_raw.len());
    process_en(&en_src_path, &wl_dir.join("en_us.txt.gz"))?;

    // Hunspell-based languages: each gets its `.dic` AND `.aff`,
    // then the affix expander turns ~350k stems into ~1-3M surface
    // forms (the ones a user actually types). Each language is
    // independent — a transient 404 / 5xx on one source shouldn't
    // take the whole script down.
    fetch_hunspell(
        &src_dir,
        &wl_dir,
        "uk_UA",
        UK_DIC_URL,
        UK_AFF_URL,
        "uk_ua.txt.gz",
    );
    // uk_UA additionally ships a README that we keep next to the
    // sources for license-attribution purposes.
    if let Ok(readme) = http_get(UK_README_URL) {
        let p = src_dir.join("uk_UA-README.txt");
        let _ = fs::write(&p, &readme);
        println!("  saved {} ({} bytes)", p.display(), readme.len());
    }
    fetch_hunspell(
        &src_dir,
        &wl_dir,
        "ru_RU",
        RU_DIC_URL,
        RU_AFF_URL,
        "ru_ru.txt.gz",
    );
    fetch_hunspell(
        &src_dir,
        &wl_dir,
        "de_DE_frami",
        DE_DIC_URL,
        DE_AFF_URL,
        "de_de.txt.gz",
    );
    fetch_hunspell(
        &src_dir,
        &wl_dir,
        "es_ES",
        ES_DIC_URL,
        ES_AFF_URL,
        "es_es.txt.gz",
    );
    fetch_hunspell(
        &src_dir,
        &wl_dir,
        "fr",
        FR_DIC_URL,
        FR_AFF_URL,
        "fr_fr.txt.gz",
    );

    println!("\nDone. Review with `git diff data/wordlists/` and commit.");
    Ok(())
}

/// Download one language's `.dic` AND `.aff`, drop them under
/// `sources/<base>.dic` / `sources/<base>.aff`, then run the affix
/// expander to produce `<wl_dir>/<output>` containing all surface
/// forms.
///
/// Errors are surfaced on stderr but don't abort the rest of the
/// fetch run — partial progress is better than none for a multi-
/// source script. `base` is the upstream source-file stem (e.g.
/// `"uk_UA"`, `"de_DE_frami"`, `"fr"`); `output` is whatever
/// filename we want in `wl_dir` (which we keep snake_case for
/// consistency: `uk_ua.txt`, `de_de.txt`, `fr_fr.txt`).
pub(crate) fn fetch_hunspell(
    src_dir: &Path,
    wl_dir: &Path,
    base: &str,
    dic_url: &str,
    aff_url: &str,
    output: &str,
) {
    let dic_path = src_dir.join(format!("{base}.dic"));
    let aff_path = src_dir.join(format!("{base}.aff"));

    if let Err(e) = download(dic_url, &dic_path) {
        eprintln!("  {e}");
        return;
    }
    if let Err(e) = download(aff_url, &aff_path) {
        eprintln!("  {e}");
        return;
    }

    let out = wl_dir.join(output);
    if let Err(e) = process_hunspell_with_aff(&dic_path, &aff_path, &out) {
        eprintln!("  process {} failed: {e}", dic_path.display());
    }
}

pub(crate) fn download(url: &str, dest: &Path) -> Result<()> {
    println!("Downloading {url}");
    let raw = http_get(url).with_context(|| format!("download {url}: HTTP request failed"))?;
    fs::write(dest, &raw).with_context(|| format!("write {} failed", dest.display()))?;
    println!("  saved {} ({} bytes)", dest.display(), raw.len());
    Ok(())
}

/// Read a Hunspell `.aff` or `.dic` and decode to UTF-8.
///
/// Most modern LibreOffice dictionaries (`uk_UA`, `ru_RU`, `es_ES`,
/// `fr`) ship as UTF-8. The German `de_DE_frami` files are still
/// ISO-8859-1 — the file declares `SET ISO8859-1` and the umlauts
/// in conditions like `[äöü]` are encoded as single high bytes. To
/// keep one code path:
///
/// 1. Try parsing as UTF-8. ~95 % of files take this fast lane.
/// 2. On failure, look for the `SET <encoding>` directive in the
///    first 2 KB (decoded byte-by-byte first as Latin-1 so the scan
///    itself works).
/// 3. If the SET says `ISO8859*` / `LATIN1` / `WINDOWS-1252`, decode
///    every byte as the corresponding Latin-1 codepoint
///    (`U+0000..=U+00FF`). For our dictionaries this is exact —
///    Windows-1252 differs from Latin-1 only in 0x80-0x9F, which our
///    sources don't use.
/// 4. If the SET is missing, default to Latin-1 — better than an
///    error, and `cargo xtask wordlists fetch` will print the file
///    name in the build log either way.
pub(crate) fn read_hunspell_text(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    if let Ok(s) = std::str::from_utf8(&bytes) {
        return Ok(s.to_string());
    }

    // Latin-1 view of the first 2 KB — enough to find a SET line.
    let preview: String = bytes.iter().take(2048).map(|&b| b as char).collect();
    let declared = preview
        .lines()
        .filter_map(|l| l.split('#').next())
        .find_map(|l| l.trim().strip_prefix("SET "))
        .map(|s| s.trim().to_uppercase());

    match declared.as_deref() {
        Some("UTF-8") => bail!(
            "{} declares `SET UTF-8` but its bytes aren't valid UTF-8",
            path.display()
        ),
        Some(enc)
            if enc.starts_with("ISO8859")
                || enc.starts_with("ISO-8859")
                || enc == "LATIN1"
                || enc == "WINDOWS-1252" =>
        {
            Ok(bytes.iter().map(|&b| b as char).collect())
        }
        Some(other) => bail!(
            "{} uses unsupported `SET {other}`; add it to read_hunspell_text",
            path.display()
        ),
        None => Ok(bytes.iter().map(|&b| b as char).collect()),
    }
}

pub(crate) fn process_en(input: &Path, output: &Path) -> Result<()> {
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

/// Hunspell `.dic` + `.aff` → expanded surface-form list.
///
/// 1. Parse the `.aff` (one-time per language).
/// 2. For each `.dic` entry `<stem>/<flags>`, run the expander to
///    produce all surface forms (the stem itself plus every form the
///    rules generate from those flags).
/// 3. Lowercase, filter to entries that read as words (letters +
///    apostrophe + hyphen, at least one alphabetic char), dedupe via
///    `BTreeSet`, write sorted.
///
/// Same cleanup pipeline as the previous "strip flags only" path —
/// the difference is that `set` now contains `1M+` entries instead
/// of `350k` for inflected languages. FST encoding makes the on-disk
/// size grow ~3-5×, which is fine for our embed budget.
pub(crate) fn process_hunspell_with_aff(dic: &Path, aff: &Path, output: &Path) -> Result<()> {
    let aff_text = read_hunspell_text(aff)?;
    let parsed =
        hunspell::Aff::parse(&aff_text).with_context(|| format!("parse {}", aff.display()))?;

    let dic_text = read_hunspell_text(dic)?;
    let mut words: BTreeSet<String> = BTreeSet::new();
    let mut stem_count = 0usize;
    let mut iter = dic_text.lines();
    let _count = iter.next(); // first Hunspell line is the entry count

    for line in iter {
        let line = line.trim();
        if line.is_empty() || line.starts_with('+') || line.starts_with('#') {
            continue;
        }
        let (stem, flags) = match line.split_once('/') {
            Some((s, f)) => (s.trim(), f.trim()),
            None => (line, ""),
        };
        if stem.is_empty() {
            continue;
        }
        stem_count += 1;
        for form in parsed.expand(stem, flags) {
            let lower: String = form.chars().flat_map(char::to_lowercase).collect();
            let acceptable = lower
                .chars()
                .all(|c| c.is_alphabetic() || matches!(c, '\'' | '-' | 'ʼ' | '\u{2019}'));
            let has_letter = lower.chars().any(|c| c.is_alphabetic());
            if acceptable && has_letter {
                words.insert(lower);
            }
        }
    }

    write_sorted(output, &words)?;
    let name = output.file_name().and_then(|s| s.to_str()).unwrap_or("?");
    println!(
        "  {name}: {} forms (expanded from {stem_count} stems via .aff rules)",
        words.len()
    );
    Ok(())
}

/// Write `words` sorted, one per line, to `path`.
///
/// Gzip-aware: if `path` ends in `.gz` the output stream is wrapped
/// in `GzEncoder`. Cuts the on-disk footprint of the bulk wordlists
/// by ~5× (uk_ua is 84 MB raw, ~25 MB gzipped) which keeps the
/// repo's first-clone size under control.
pub(crate) fn write_sorted(path: &Path, words: &BTreeSet<String>) -> Result<()> {
    let file = File::create(path).with_context(|| format!("create {}", path.display()))?;
    let is_gz = path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("gz"));
    let mut w: Box<dyn Write> = if is_gz {
        // Default `Compression::default()` is level 6 — same trade-off
        // GitHub's CDN uses on its own gzipped responses.
        Box::new(BufWriter::new(flate2::write::GzEncoder::new(
            file,
            flate2::Compression::default(),
        )))
    } else {
        Box::new(BufWriter::new(file))
    };
    for word in words {
        writeln!(w, "{word}")?;
    }
    Ok(())
}

pub(crate) fn http_get(url: &str) -> Result<Vec<u8>> {
    let resp = ureq::get(url)
        .timeout(std::time::Duration::from_secs(120))
        .call()?;
    let mut buf = Vec::new();
    resp.into_reader().read_to_end(&mut buf)?;
    Ok(buf)
}
