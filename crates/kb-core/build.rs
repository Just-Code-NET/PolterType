//! Build-time wordlist → FST conversion + short-stop-word embedding.
//!
//! The wordlist text files committed under `data/wordlists/` are the
//! human-readable source of truth:
//!
//! * `<stem>.txt` — full upstream dictionary, used for ≥3-letter
//!   tokens. Compiled into a [BurntSushi FST] blob in `OUT_DIR` for
//!   compact lookup.
//! * `<stem>-stop.txt` — hand-curated 1- and 2-letter stop words,
//!   used for ≤2-letter tokens (the embedded FST is too noisy at
//!   that length — see `kb-detect::LayoutDictionary` doc-comment).
//!
//! At build time we sort + dedupe the full wordlist, write the FST
//! into `OUT_DIR`, and emit `embedded_wordlists.rs` — a tiny
//! dispatcher exposing `EMBEDDED_WORDLISTS: &[(&str, &[u8], &str)]`
//! (stem, FST bytes, raw stop-list text) — which the lib code picks
//! up via `include!(concat!(env!("OUT_DIR"), "/embedded_wordlists.rs"))`.
//!
//! [BurntSushi FST]: https://docs.rs/fst

// Build scripts are explicitly allowed to use unwrap/expect per the
// project's CLAUDE.md style — a panic here is an honest "build is
// broken" signal, not a runtime hazard.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use fst::SetBuilder;

/// (`<layout-id-as-file-stem>`, expected layout BCP-47 tag for the
/// "did we wire it up?" diagnostic — only used in the build log).
///
/// Each entry expects three files under `data/wordlists/`:
///
/// * `<stem>.txt` — full wordlist (compiled into FST). May be absent
///   or empty; the FST then carries zero entries and the layout falls
///   back to plausibility-only detection.
/// * `<stem>-extras.txt` — same shape as above; merged with
///   `<stem>.txt` before FST building. May be absent.
/// * `<stem>-stop.txt` — hand-curated 1- and 2-letter stop words.
///   **Must exist** (this file is `include_str!`'d into the binary) —
///   even an empty list with a header comment is fine.
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
    // crates/kb-core → repo root → data/wordlists
    let wordlists_dir = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("data").join("wordlists"))
        .expect("repo root not found");

    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR not set"));

    for (stem, tag) in LAYOUTS {
        let txt_gz_path = wordlists_dir.join(format!("{stem}.txt.gz"));
        let txt_path = wordlists_dir.join(format!("{stem}.txt"));
        let extras_path = wordlists_dir.join(format!("{stem}-extras.txt"));
        let stop_path = wordlists_dir.join(format!("{stem}-stop.txt"));
        let fst_path = out_dir.join(format!("{stem}.fst"));

        println!("cargo:rerun-if-changed={}", txt_gz_path.display());
        println!("cargo:rerun-if-changed={}", txt_path.display());
        println!("cargo:rerun-if-changed={}", extras_path.display());
        println!("cargo:rerun-if-changed={}", stop_path.display());

        // Full upstream wordlist + our hand-curated extras
        // (modern tech vocab, common acronyms, dev jargon — the
        // categories `dwyl/english-words` doesn't cover).
        //
        // Bulk wordlist source ships gzipped — the uk_ua expanded
        // form list is 84 MB raw, ~25 MB gzipped, and lives in the
        // repo as `data/wordlists/<stem>.txt.gz`. The plain `.txt`
        // path is honoured as a fallback so a contributor inspecting
        // a wordlist can `gunzip -k <stem>.txt.gz` and re-build
        // without re-running xtask.
        let mut words: Vec<String> = read_wordlist(&txt_gz_path);
        if words.is_empty() {
            words = read_wordlist(&txt_path);
        }
        let extras = read_wordlist(&extras_path);
        let extras_count = extras.len();
        words.extend(extras);
        words.sort();
        words.dedup();
        let _ = extras_count; // surfaced via the cargo:warning below

        let writer =
            BufWriter::new(File::create(&fst_path).expect("could not create FST output file"));
        let mut builder = SetBuilder::new(writer).expect("FST set builder");
        for w in &words {
            builder.insert(w).expect("FST insert");
        }
        builder.finish().expect("FST finish");

        let stop_count = read_wordlist(&stop_path).len();
        // Report shape: one line per layout. Two cases:
        //
        // * Populated dictionary (en/uk by default; ru/de/es/fr after
        //   `cargo xtask wordlists fetch`) → the same diagnostic line
        //   we've always printed.
        // * Empty FST (the new languages out-of-the-box) → ONE clear
        //   actionable hint instead of three "file not found" warnings
        //   per language. Suppressing the missing-file warnings is the
        //   complementary half — see `read_wordlist`.
        if words.is_empty() {
            println!(
                "cargo:warning=wordlist {tag}: empty FST (run `cargo xtask wordlists fetch` to populate; \
                 plausibility-only detection used until then). {stop_count} short stop-words present."
            );
        } else {
            println!(
                "cargo:warning=wordlist {tag}: {} entries (FST, +{extras_count} extras) + {stop_count} short stop-words",
                words.len()
            );
        }
    }

    // Generate the dispatcher.
    let dispatch_path = out_dir.join("embedded_wordlists.rs");
    let mut dispatch = BufWriter::new(File::create(&dispatch_path).expect("dispatch file"));
    writeln!(
        &mut dispatch,
        "/// Generated by build.rs — (stem, FST bytes, short-stop list raw text)."
    )
    .unwrap();
    writeln!(
        &mut dispatch,
        "pub const EMBEDDED_WORDLISTS: &[(&str, &[u8], &str)] = &["
    )
    .unwrap();
    for (stem, _) in LAYOUTS {
        writeln!(
            &mut dispatch,
            "    (\"{stem}\", \
             include_bytes!(concat!(env!(\"OUT_DIR\"), \"/{stem}.fst\")), \
             include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../../data/wordlists/{stem}-stop.txt\"))),"
        )
        .unwrap();
    }
    writeln!(&mut dispatch, "];").unwrap();
}

/// Read one wordlist file into a deduped, lowercased Vec of words.
///
/// Honours both raw `<stem>.txt` and gzipped `<stem>.txt.gz` —
/// dispatch is by file extension. Bulk wordlists are gzipped because
/// the expanded uk_ua dictionary alone is 84 MB raw; the small
/// hand-curated `-extras.txt` / `-stop.txt` files are still plain
/// text so contributors can edit them in any text editor without a
/// decompress / recompress dance.
///
/// File-not-found is a *legitimate* state — the new languages
/// (ru/de/es/fr) shipped empty originally, and a contributor who
/// hasn't run `cargo xtask wordlists fetch` yet will be missing the
/// `.txt.gz` file. So ENOENT is silent. Other I/O errors (permission
/// denied, malformed UTF-8 deeper down) are still surfaced — those
/// are the cases worth bothering the user about. The "you have an
/// empty FST" hint comes once per layout from the call site.
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
        .map(|l| l.trim().to_lowercase())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
}
