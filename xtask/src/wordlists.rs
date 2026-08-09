//! Wordlist pipeline: download, Hunspell-expand, write sorted.

#[cfg(test)]
mod tests;

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
    let mut failed: Vec<&str> = Vec::new();
    for source in HUNSPELL_SOURCES {
        if !fetch_hunspell(&src_dir, &wl_dir, source) {
            failed.push(source.output);
        }
    }

    // uk_UA additionally ships a README that we keep next to the
    // sources for license-attribution purposes.
    if let Ok(readme) = http_get(UK_README_URL) {
        let p = src_dir.join("uk_UA-README.txt");
        let _ = fs::write(&p, &readme);
        println!("  saved {} ({} bytes)", p.display(), readme.len());
    }

    // A dead upstream URL used to cost one stderr line and still exit
    // 0, leaving whatever stale `.txt.gz` was already committed in
    // place — which is exactly how the French source sat broken
    // unnoticed. Per-language recovery is still the right behaviour,
    // so we keep going and report at the end; the exit code is what
    // changes.
    if !failed.is_empty() {
        bail!(
            "{} of {} Hunspell sources failed: {}. \
             Their previously committed wordlists were left untouched — \
             check the URLs in xtask/src/consts.rs against upstream.",
            failed.len(),
            HUNSPELL_SOURCES.len(),
            failed.join(", ")
        );
    }

    println!("\nDone. Review with `git diff data/wordlists/` and commit.");
    Ok(())
}

/// Download one language's `.dic` and `.aff` into `sources/`, then run
/// the affix expander to produce `<wl_dir>/<output>` with all surface
/// forms.
///
/// Errors go to stderr without aborting the rest of the run — partial
/// progress beats none for a multi-source script. Returns whether this
/// language came through, so the caller can still fail the command.
pub(crate) fn fetch_hunspell(src_dir: &Path, wl_dir: &Path, source: &HunspellSource) -> bool {
    let dic_path = src_dir.join(format!("{}.dic", source.base));
    let aff_path = src_dir.join(format!("{}.aff", source.base));

    if let Err(e) = download(source.dic, &dic_path) {
        eprintln!("  {e}");
        return false;
    }
    if let Err(e) = download(source.aff, &aff_path) {
        eprintln!("  {e}");
        return false;
    }

    let out = wl_dir.join(source.output);
    if let Err(e) = process_hunspell_with_aff(&dic_path, &aff_path, &out, source.expand) {
        eprintln!("  process {} failed: {e}", dic_path.display());
        return false;
    }
    true
}

pub(crate) fn download(url: &str, dest: &Path) -> Result<()> {
    println!("Downloading {url}");
    let raw = http_get(url).with_context(|| format!("download {url}: HTTP request failed"))?;
    fs::write(dest, &raw).with_context(|| format!("write {} failed", dest.display()))?;
    println!("  saved {} ({} bytes)", dest.display(), raw.len());
    Ok(())
}

/// Read the `SET <encoding>` directive out of a `.aff`.
///
/// **This is the encoding of the whole dictionary pair, `.dic`
/// included.** Hunspell declares it once, in the `.aff`. Reading each
/// file's own bytes and falling back to Latin-1 is how Polish and Greek
/// shipped as mojibake: the `.aff` decoded correctly, the `.dic` did
/// not, and nothing failed. German came through only because German
/// *is* Latin-1.
///
/// An unrecognised or absent `SET` is an error rather than a guess, for
/// the same reason.
pub(crate) fn detect_encoding(aff_path: &Path) -> Result<Encoding> {
    let bytes = fs::read(aff_path).with_context(|| format!("read {}", aff_path.display()))?;
    encoding_of_aff(&bytes)
        .with_context(|| format!("determining the encoding of {}", aff_path.display()))
}

/// The byte-level half of [`detect_encoding`], split out so it can be
/// tested without touching the filesystem.
pub(crate) fn encoding_of_aff(bytes: &[u8]) -> Result<Encoding> {
    // A UTF-8 BOM would otherwise glue itself to the front of the very
    // first line and hide a `SET` sitting there — pt_BR ships one.
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);

    // Latin-1 view of the first 2 KB — enough to find a SET line, and
    // safe on any byte, which matters because we don't yet know the
    // encoding we're looking for.
    let preview: String = bytes.iter().take(2048).map(|&b| b as char).collect();
    let declared = preview
        .lines()
        .filter_map(|l| l.split('#').next())
        .find_map(|l| l.trim().strip_prefix("SET "))
        .map(|s| s.trim().to_uppercase());

    let normalized = declared
        .as_deref()
        .map(|e| e.replace("ISO-8859", "ISO8859"));
    match normalized.as_deref() {
        Some("UTF-8") => Ok(Encoding::Utf8),
        Some("ISO8859-1" | "LATIN1" | "WINDOWS-1252") => Ok(Encoding::Latin1),
        Some("ISO8859-2" | "LATIN2") => Ok(Encoding::Latin2),
        Some("ISO8859-7") => Ok(Encoding::Greek),
        Some(other) => bail!(
            "declares `SET {other}`, which this expander cannot decode. \
             Add the codepage to xtask/src/consts.rs and a variant to \
             `Encoding` — do NOT let it fall through to Latin-1, that is \
             what silently mangled Polish and Greek."
        ),
        None => bail!(
            "no `SET <encoding>` directive, so the encoding of the matching \
             .dic is unknown. Guessing here corrupts every non-ASCII word \
             without failing; add an explicit SET or extend \
             `encoding_of_aff`."
        ),
    }
}

/// Read a Hunspell `.aff` or `.dic` and decode it with the encoding
/// [`detect_encoding`] found in the pair's `.aff`.
pub(crate) fn read_hunspell_text(path: &Path, encoding: Encoding) -> Result<String> {
    let raw = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    // Same BOM as `encoding_of_aff` skips. Left in place it becomes a
    // U+FEFF on the front of the first line — which `str::trim` does
    // not remove, so it would quietly corrupt the first `.dic` entry.
    let bytes = raw.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&raw);
    match encoding {
        Encoding::Utf8 => std::str::from_utf8(bytes)
            .map(str::to_owned)
            .with_context(|| format!("{} declares UTF-8 but isn't valid UTF-8", path.display())),
        // Every ISO-8859-N agrees with Unicode below 0xA0; only the
        // top 96 bytes need a table, and Latin-1's is the identity.
        Encoding::Latin1 => Ok(bytes.iter().map(|&b| b as char).collect()),
        Encoding::Latin2 => Ok(decode_high(bytes, &LATIN2_HIGH)),
        Encoding::Greek => Ok(decode_high(bytes, &GREEK_HIGH)),
    }
}

/// Decode a single-byte codepage: `0x00..0xA0` pass through as their
/// own code point, `0xA0..=0xFF` come from `high`.
fn decode_high(bytes: &[u8], high: &[char; 96]) -> String {
    bytes
        .iter()
        .map(|&b| {
            if b < 0xA0 {
                b as char
            } else {
                high[usize::from(b) - 0xA0]
            }
        })
        .collect()
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
/// Parse the `.aff` once, expand every `<stem>/<flags>` entry into its
/// surface forms, then lowercase, filter to entries that read as words,
/// dedupe through a `BTreeSet` and write sorted.
///
/// Same cleanup pipeline as the older strip-flags-only path; the
/// difference is 1M+ entries instead of 350k for inflected languages,
/// which FST encoding grows ~3-5× on disk — within the embed budget.
///
/// [`ExpandMode::StemsOnly`] skips the expansion and keeps bare stems;
/// see the enum for the one dictionary that needs it.
pub(crate) fn process_hunspell_with_aff(
    dic: &Path,
    aff: &Path,
    output: &Path,
    mode: ExpandMode,
) -> Result<()> {
    // The .aff declares the encoding for both halves of the pair.
    let encoding = detect_encoding(aff)?;
    let aff_text = read_hunspell_text(aff, encoding)?;
    let parsed =
        hunspell::Aff::parse(&aff_text).with_context(|| format!("parse {}", aff.display()))?;

    let dic_text = read_hunspell_text(dic, encoding)?;
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
        let forms = match mode {
            ExpandMode::Full => parsed.expand(stem, flags),
            ExpandMode::StemsOnly => std::iter::once(stem.to_owned()).collect(),
        };
        for form in forms {
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
    let how = match mode {
        ExpandMode::Full => format!("expanded from {stem_count} stems via .aff rules"),
        ExpandMode::StemsOnly => format!("{stem_count} stems, affix expansion skipped"),
    };
    println!("  {name}: {} forms ({how})", words.len());
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
