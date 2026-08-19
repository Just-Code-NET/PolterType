//! Build-time data preparation: turn the committed `data/` inputs into
//! the on-disk `data/` tree poltertype loads at runtime — FSTs from the
//! gzipped wordlists, plus verbatim copies of the stop lists and layout
//! mappings. `docs/DATA_LAYOUT.md` has the full input/output map.
//!
//! Output goes to `<workspace>/target/dist/data/` rather than `OUT_DIR`
//! because the installers need a stable path at packaging time, and it
//! is what the dev-mode runtime resolver falls back to. Cargo's
//! preference for `OUT_DIR` is about crates consumed by others.
//!
//! Every input is declared with `cargo:rerun-if-changed`, so an
//! unchanged tree does no work — see `declare_inputs` for the trap.

// Build scripts are explicitly allowed to use unwrap/expect/panic per
// the project's style — a panic here is an honest "build is
// broken" signal, not a runtime hazard.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use fst::SetBuilder;

/// (`<layout-id-as-file-stem>`, BCP-47 tag for the build-log
/// diagnostic). Keep in lock-step with the runtime layout list —
/// discrepancies surface as missing-layout warnings at startup.
const LAYOUTS: &[(&str, &str)] = &[
    ("en_us", "en-US"),
    ("uk_ua", "uk-UA"),
    ("ru_ru", "ru-RU"),
    ("de_de", "de-DE"),
    ("es_es", "es-ES"),
    ("fr_fr", "fr-FR"),
    ("pl_pl", "pl-PL"),
    ("cs_cz", "cs-CZ"),
    ("el_gr", "el-GR"),
    ("he_il", "he-IL"),
    ("tr_tr", "tr-TR"),
    ("bg_bg", "bg-BG"),
    ("it_it", "it-IT"),
    ("pt_pt", "pt-PT"),
    ("pt_br", "pt-BR"),
];

fn main() {
    // Runtime env, NOT the `env!` macro: the macro bakes the path into
    // the compiled build-script binary, so a moved/renamed checkout
    // keeps reading data from the old location for as long as cargo
    // considers the cached script fresh.
    let manifest_dir =
        PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"));
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
    //
    // The directory is watched as a whole so a wordlist added later
    // still triggers a rebuild. It has to be here, not in
    // `prepare_wordlist`, which declares only files that exist.
    println!("cargo:rerun-if-changed={}", src_wordlists.display());
    for (stem, tag) in LAYOUTS {
        prepare_wordlist(&src_wordlists, &out_wordlists, stem, tag);
    }

    // ─── UI translations: copy catalogs ────────────────────────────
    // Whole-directory copy rather than a list: catalogs are pure data
    // with no build step, and a contributor adding `pl.toml` should
    // not also have to edit a Rust file to make it ship.
    let src_i18n = repo_root.join("data").join("i18n");
    let out_i18n = out_root.join("i18n");
    fs::create_dir_all(&out_i18n).expect("mkdir target/dist/data/i18n");
    println!("cargo:rerun-if-changed={}", src_i18n.display());
    match fs::read_dir(&src_i18n) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                    continue;
                }
                println!("cargo:rerun-if-changed={}", path.display());
                if let Some(name) = path.file_name() {
                    if let Err(e) = fs::copy(&path, out_i18n.join(name)) {
                        println!("cargo:warning=i18n copy {} failed: {e}", path.display());
                    }
                }
            }
        }
        Err(e) => println!("cargo:warning=no data/i18n directory ({e}); UI stays English"),
    }

    // ─── Layout mappings: copy TOMLs ───────────────────────────────
    for (stem, _) in LAYOUTS {
        let src = src_mappings.join(format!("{stem}.toml"));
        let dst = out_mappings.join(format!("{stem}.toml"));
        println!("cargo:rerun-if-changed={}", src.display());
        if let Err(e) = fs::copy(&src, &dst) {
            // Missing TOML warns rather than panics: the runtime simply
            // won't see that mapping.
            println!(
                "cargo:warning=mapping copy {} → {} failed: {e}",
                src.display(),
                dst.display()
            );
        }
    }
}

/// Locate the workspace's `target/` dir by walking up from `OUT_DIR`,
/// which cargo guarantees lives under it. Survives the default layout,
/// `CARGO_TARGET_DIR` overrides and a custom `[build.target-dir]`.
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

    // ONLY paths that exist. A `rerun-if-changed` pointing at a missing
    // file makes cargo treat this script as stale on every invocation,
    // and every crate in the workspace depends on this one — four of the
    // five names below are absent for nearly every language, which cost
    // 128 s on a no-op run. Creating one is still caught: the caller
    // declares the containing directory, and cargo scans it.
    for path in [&txt_gz, &txt, &extras, &stop, &weak] {
        if path.exists() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }

    // The bulk source ships gzipped (uk_ua is 84 MB raw, ~10 MB
    // gzipped); plain `.txt` is honoured so a contributor can
    // `gunzip -k` and rebuild. Each entry is normalised twice in one
    // pass — the lossy `letters_only_lower` shape for the membership
    // FST and `surface_lower` for the suggestions FST, which keeps
    // apostrophes and hyphens because a suggestion is typed into the
    // user's text.
    let (mut words, mut surface_words) = read_wordlist_both(&txt_gz);
    if words.is_empty() {
        (words, surface_words) = read_wordlist_both(&txt);
    }
    let (extras_words, extras_surface) = read_wordlist_both(&extras);
    let extras_count = extras_words.len();
    // Carve the 1- and 2-letter entries out of the curated extras
    // *before* they fold into the bulk FST: the runtime short-token
    // lookup skips the FST deliberately, so a 2-letter acronym living
    // only there is invisible to it. These are our own curated list, so
    // mirroring their short subset into the stop file is safe. Without
    // it, `AI` under uk-UA renders `ФШ` and no detector has a signal.
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

    // Surface-form FST for the suggestions engine. ≤2-letter entries
    // are dropped: suggestions never target short tokens, and the bulk
    // corpora are noisy at that length (same rationale as the runtime
    // short-token regime skipping the membership FST).
    surface_words.extend(extras_surface);
    surface_words.retain(|w| w.chars().filter(|c| c.is_alphabetic()).count() >= 3);
    surface_words.sort();
    surface_words.dedup();
    let surface_path = out_dir.join(format!("{stem}-surface.fst"));
    if surface_words.is_empty() {
        let _ = fs::remove_file(&surface_path);
    } else {
        let writer = BufWriter::new(File::create(&surface_path).expect("create surface FST"));
        let mut builder = SetBuilder::new(writer).expect("surface FST set builder");
        for w in &surface_words {
            builder.insert(w).expect("surface FST insert");
        }
        builder.finish().expect("surface FST finish");
    }

    // Compose the dist stop-words file: the source verbatim (comments
    // included — the runtime parser ignores them) plus `short_extras`,
    // so the runtime loader has nothing to compose of its own.
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
            "cargo:warning=wordlist {tag}: {} entries (FST, +{extras_count} extras) + {} surface forms + {stop_count} short stop-words + {weak_count} weak entries",
            words.len(),
            surface_words.len()
        );
    }
}

/// Write the dist `<stem>-stop.txt`: the source stop file verbatim —
/// comments and ordering survive — plus an auto-generated section with
/// the ≤2-letter entries from `<stem>-extras.txt`. Returns how many
/// unique short stop-words the runtime will load.
///
/// Dedup is against `source_text`, so an acronym hand-added to the stop
/// file is not duplicated when extras carries it too.
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

/// Read one wordlist file into a deduped, lowercased `Vec`, dispatching
/// on extension between raw and gzipped. File-not-found is silent —
/// most languages ship without a `-extras` file; other I/O errors
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

/// Like [`read_wordlist`], but produces both normalisations in one pass
/// over a potentially multi-million-line source. One pass matters:
/// gunzipping uk_ua twice would double the slowest step of this script.
fn read_wordlist_both(path: &Path) -> (Vec<String>, Vec<String>) {
    let f = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Vec::new(), Vec::new()),
        Err(e) => {
            println!(
                "cargo:warning=wordlist read failed for {}: {e}",
                path.display()
            );
            return (Vec::new(), Vec::new());
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
    let mut letters = Vec::new();
    let mut surface = Vec::new();
    for l in lines.lines().map_while(Result::ok) {
        let trimmed = l.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let normalized = letters_only_lower(trimmed);
        if normalized.is_empty() {
            continue;
        }
        letters.push(normalized);
        surface.push(surface_lower(trimmed));
    }
    (letters, surface)
}

/// Mirror of `poltertype_detect::letters_only_lower`, duplicated because
/// build scripts cannot depend on workspace crates without inflating
/// build-time deps. The FST and the runtime lookup must be built
/// against the same shape — keep the two in sync.
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

/// Mirror of `poltertype_detect::surface_lower`: lowercase, keep letters
/// plus apostrophes and hyphens, fold `’` and `ʼ` to `'`. Keep the two
/// in sync — the suggester queries this FST with tokens canonicalised
/// by the runtime twin.
fn surface_lower(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        // Apostrophes first: `ʼ` (U+02BC) is Unicode category Lm —
        // `is_alphabetic()` returns true for it, so the alphabetic
        // branch would keep it un-folded.
        if matches!(ch, '\'' | '’' | 'ʼ') {
            out.push('\'');
        } else if ch == '-' {
            out.push('-');
        } else if ch.is_alphabetic() {
            for low in ch.to_lowercase() {
                out.push(low);
            }
        }
    }
    out
}
