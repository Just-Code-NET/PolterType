//! Build-time data preparation: produce the on-disk `data/` tree that
//! poltertype loads at runtime.
//!
//! ## What lands where
//!
//! Inputs (committed in the repo, in `<workspace>/data/`):
//!
//! * `data/wordlists/<stem>.txt.gz` — bulk Hunspell-grade dictionary
//!   for ≥3-letter tokens. Empty / absent → no FST for that stem
//!   (plausibility-only detection still works).
//! * `data/wordlists/<stem>-extras.txt` — hand-curated additions.
//! * `data/wordlists/<stem>-stop.txt` — curated 1- / 2-letter stop
//!   words.
//! * `data/layout-mappings/<stem>.toml` — keyboard mapping.
//!
//! Outputs (built fresh each `cargo build`, under
//! `<workspace>/target/dist/data/`):
//!
//! ```text
//! target/dist/data/
//!   wordlists/
//!     <stem>.fst                  ← FST built from .txt.gz + extras
//!     <stem>-stop.txt             ← copied as-is
//!   layout-mappings/
//!     <stem>.toml                 ← copied as-is
//! ```
//!
//! Why a stable path instead of `OUT_DIR`: the installers (WiX MSI,
//! AppImage AppDir, macOS .app) need to know *where* to copy these
//! files at packaging time. `OUT_DIR` is per-crate-hash and changes
//! every cargo invocation — useless for that. `target/dist/data` is
//! invariant, predictable, and matches the dev-mode runtime
//! resolver in `poltertype-core::data_dir` (the dev fallback path).
//!
//! Cargo prefers build scripts to write only inside `OUT_DIR`; the
//! warning is for portability of crates *consumed by others*. We are
//! a workspace's own crate writing to its own workspace's own target
//! dir — fully under our control.
//!
//! ## Idempotency / dev experience
//!
//! `cargo:rerun-if-changed=` lines mark every input. So:
//!
//! * Edit `data/wordlists/uk_ua-extras.txt` → only uk_ua FST rebuilds.
//! * Tweak a TOML mapping → only that TOML is recopied.
//! * Plain `cargo build` after no source changes → no work, no
//!   warnings.

// Build scripts are explicitly allowed to use unwrap/expect/panic per
// the project's CLAUDE.md style — a panic here is an honest "build is
// broken" signal, not a runtime hazard.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use fst::SetBuilder;

/// (`<layout-id-as-file-stem>`, BCP-47 tag for the build-log
/// "did we wire it up?" diagnostic).
///
/// Keep in lock-step with the runtime layout list — discrepancies
/// surface as missing-layout warnings at startup, never silent.
const LAYOUTS: &[(&str, &str)] = &[
    ("en_us", "en-US"),
    ("uk_ua", "uk-UA"),
    ("ru_ru", "ru-RU"),
    ("de_de", "de-DE"),
    ("es_es", "es-ES"),
    ("fr_fr", "fr-FR"),
];

fn main() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // crates/poltertype-core → repo root
    let repo_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("repo root not found from CARGO_MANIFEST_DIR")
        .to_path_buf();

    let src_wordlists = repo_root.join("data").join("wordlists");
    let src_mappings = repo_root.join("data").join("layout-mappings");

    let out_root = workspace_target_dir().join("dist").join("data");
    let out_wordlists = out_root.join("wordlists");
    let out_mappings = out_root.join("layout-mappings");

    fs::create_dir_all(&out_wordlists).expect("mkdir target/dist/data/wordlists");
    fs::create_dir_all(&out_mappings).expect("mkdir target/dist/data/layout-mappings");

    // ─── Wordlists: build FST + copy stop-word list ────────────────
    for (stem, tag) in LAYOUTS {
        prepare_wordlist(&src_wordlists, &out_wordlists, stem, tag);
    }

    // ─── Layout mappings: copy TOMLs ───────────────────────────────
    for (stem, _) in LAYOUTS {
        let src = src_mappings.join(format!("{stem}.toml"));
        let dst = out_mappings.join(format!("{stem}.toml"));
        println!("cargo:rerun-if-changed={}", src.display());
        if let Err(e) = fs::copy(&src, &dst) {
            // Don't panic on missing TOML — same forgiving spirit as
            // the wordlist path. If a user removes a mapping the
            // runtime simply won't see it.
            println!(
                "cargo:warning=mapping copy {} → {} failed: {e}",
                src.display(),
                dst.display()
            );
        }
    }
}

/// Locate the workspace's `target/` dir. We deduce it from `OUT_DIR`,
/// which cargo guarantees is somewhere under `target`. Walking up to
/// the first ancestor named `target` is reliable across:
///
/// * default layout (`<workspace>/target/...`)
/// * `CARGO_TARGET_DIR` overrides
/// * cargo workspaces with custom `[build.target-dir]`
fn workspace_target_dir() -> PathBuf {
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR not set"));
    for ancestor in out_dir.ancestors() {
        if ancestor.file_name().is_some_and(|n| n == "target") {
            return ancestor.to_path_buf();
        }
    }
    panic!(
        "could not deduce workspace target dir from OUT_DIR={}",
        out_dir.display()
    );
}

/// Build `<stem>.fst` from `<stem>.txt.gz` + `<stem>-extras.txt` and
/// copy `<stem>-stop.txt` as-is. Empty / absent inputs yield an empty
/// FST and no stop-word file — both states are graceful at runtime.
fn prepare_wordlist(src_dir: &Path, out_dir: &Path, stem: &str, tag: &str) {
    let txt_gz = src_dir.join(format!("{stem}.txt.gz"));
    let txt = src_dir.join(format!("{stem}.txt"));
    let extras = src_dir.join(format!("{stem}-extras.txt"));
    let stop = src_dir.join(format!("{stem}-stop.txt"));
    let weak = src_dir.join(format!("{stem}-weak.txt"));

    println!("cargo:rerun-if-changed={}", txt_gz.display());
    println!("cargo:rerun-if-changed={}", txt.display());
    println!("cargo:rerun-if-changed={}", extras.display());
    println!("cargo:rerun-if-changed={}", stop.display());
    println!("cargo:rerun-if-changed={}", weak.display());

    // Bulk wordlist source ships gzipped (uk_ua alone is 84 MB raw,
    // ~10 MB gzipped). Plain `.txt` honoured as a fallback so a
    // contributor inspecting a wordlist can `gunzip -k` and rebuild.
    let mut words: Vec<String> = read_wordlist(&txt_gz);
    if words.is_empty() {
        words = read_wordlist(&txt);
    }
    let extras_words = read_wordlist(&extras);
    let extras_count = extras_words.len();
    // Carve out the 1- and 2-letter entries from the curated extras
    // *before* they get folded into the bulk FST. The runtime
    // short-token lookup deliberately skips the FST (the upstream
    // `dwyl/english-words` corpus ships noise like `ws` / `ax` /
    // `oe` / `ai` as 2-letter "words", which would block legitimate
    // Cyrillic switches), so a 2-letter acronym sitting only in the
    // FST is invisible to the short regime. The acronyms in
    // `<stem>-extras.txt` are *our* curated list — no noise — so
    // their short subset is safe to mirror into the short-stop
    // file. Without this, typing `AI` under uk-UA renders `ФШ` and
    // neither detector has any signal to switch on.
    let short_extras: Vec<String> = extras_words
        .iter()
        .filter(|w| w.chars().count() <= 2)
        .cloned()
        .collect();
    words.extend(extras_words);
    words.sort();
    words.dedup();

    let fst_path = out_dir.join(format!("{stem}.fst"));
    let writer = BufWriter::new(File::create(&fst_path).expect("create FST output"));
    let mut builder = SetBuilder::new(writer).expect("FST set builder");
    for w in &words {
        builder.insert(w).expect("FST insert");
    }
    builder.finish().expect("FST finish");

    // Compose the dist stop-words file: the source `<stem>-stop.txt`
    // verbatim (including comments — the runtime parser ignores them)
    // followed by the ≤2-letter extras carved out above. Writing the
    // composite ourselves (instead of `fs::copy` + a sidecar file)
    // keeps the runtime loader untouched: `read_stop_words` already
    // reads `<stem>-stop.txt` and that's where the short extras now
    // live. Missing source stop file → still emit a file containing
    // just the short extras (or remove any stale dist copy if there
    // are no short extras either).
    let stop_dst = out_dir.join(format!("{stem}-stop.txt"));
    let source_stop_text = match fs::read_to_string(&stop) {
        Ok(s) => Some(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            println!(
                "cargo:warning=stop-words read {} failed: {e}",
                stop.display()
            );
            None
        }
    };
    let stop_count = if source_stop_text.is_none() && short_extras.is_empty() {
        // Nothing to write — clear any stale dist file from a prior
        // build so `cargo clean`-less workflows stay consistent.
        let _ = fs::remove_file(&stop_dst);
        0
    } else {
        match write_stop_file(&stop_dst, source_stop_text.as_deref(), &short_extras, stem) {
            Ok(n) => n,
            Err(e) => {
                println!(
                    "cargo:warning=stop-words write {} failed: {e}",
                    stop_dst.display()
                );
                0
            }
        }
    };

    // Copy the weak-words file straight through (no extras
    // augmentation — weak entries are about *long* Hunspell-only
    // forms, the short regime never consults the FST). Missing
    // source → no destination file (runtime treats as empty list).
    let weak_dst = out_dir.join(format!("{stem}-weak.txt"));
    let weak_count = match fs::copy(&weak, &weak_dst) {
        Ok(_) => read_wordlist(&weak).len(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let _ = fs::remove_file(&weak_dst);
            0
        }
        Err(e) => {
            println!(
                "cargo:warning=weak-words copy {} → {} failed: {e}",
                weak.display(),
                weak_dst.display()
            );
            0
        }
    };

    if words.is_empty() {
        println!(
            "cargo:warning=wordlist {tag}: empty FST (run `cargo xtask wordlists fetch` to populate; \
             plausibility-only detection used until then). {stop_count} short stop-words present."
        );
    } else {
        println!(
            "cargo:warning=wordlist {tag}: {} entries (FST, +{extras_count} extras) + {stop_count} short stop-words + {weak_count} weak entries",
            words.len()
        );
    }
}

/// Write the dist `<stem>-stop.txt` from the source stop file's text
/// (preserved verbatim — comments and ordering survive) plus an
/// auto-generated section appending the ≤2-letter entries from
/// `<stem>-extras.txt`. Returns the count of unique short stop-words
/// the runtime will load.
///
/// Dedup happens against the words already present in `source_text`
/// — we don't duplicate an acronym someone hand-added to the stop
/// file even if the same token also lives in extras.
fn write_stop_file(
    dst: &Path,
    source_text: Option<&str>,
    short_extras: &[String],
    stem: &str,
) -> std::io::Result<usize> {
    use std::collections::HashSet;

    let mut existing: HashSet<String> = HashSet::new();
    if let Some(text) = source_text {
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let normalized = letters_only_lower(trimmed);
            if !normalized.is_empty() {
                existing.insert(normalized);
            }
        }
    }

    let mut to_append: Vec<&String> = short_extras
        .iter()
        .filter(|w| !existing.contains(w.as_str()))
        .collect();
    to_append.sort();
    to_append.dedup();

    let mut out = BufWriter::new(File::create(dst)?);
    if let Some(text) = source_text {
        out.write_all(text.as_bytes())?;
        if !text.ends_with('\n') {
            out.write_all(b"\n")?;
        }
    }
    if !to_append.is_empty() {
        writeln!(
            out,
            "\n# Auto-appended by build.rs from {stem}-extras.txt — \
             ≤2-letter entries are mirrored here so the short-token \
             dictionary lookup can see curated acronyms (the bulk \
             FST is intentionally skipped at this length)."
        )?;
        for w in &to_append {
            writeln!(out, "{w}")?;
        }
    }
    out.flush()?;
    Ok(existing.len() + to_append.len())
}

/// Read one wordlist file into a deduped, lowercased Vec of words.
///
/// Honours both raw `<stem>.txt` and gzipped `<stem>.txt.gz` —
/// dispatch is by file extension. File-not-found is silent (some
/// languages ship without a `-extras` file, etc.); other I/O errors
/// surface as cargo warnings.
fn read_wordlist(path: &Path) -> Vec<String> {
    let f = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            println!(
                "cargo:warning=wordlist read failed for {}: {e}",
                path.display()
            );
            return Vec::new();
        }
    };
    let lines: Box<dyn BufRead> = if path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("gz"))
    {
        Box::new(BufReader::new(GzDecoder::new(f)))
    } else {
        Box::new(BufReader::new(f))
    };
    lines
        .lines()
        .map_while(Result::ok)
        .filter_map(|l| {
            let trimmed = l.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            let normalized = letters_only_lower(trimmed);
            if normalized.is_empty() {
                None
            } else {
                Some(normalized)
            }
        })
        .collect()
}

/// Mirror of `poltertype_detect::letters_only_lower`. Duplicated here because
/// build scripts can't depend on workspace crates without inflating
/// build-time deps; the runtime dictionary lookup canonicalises typed
/// tokens the same way, so the FST + overlay must be built against
/// the same shape (no hyphens / apostrophes / digits — pure
/// lowercase letters). Keep the two in sync.
fn letters_only_lower(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch.is_alphabetic() {
            for low in ch.to_lowercase() {
                out.push(low);
            }
        }
    }
    out
}
