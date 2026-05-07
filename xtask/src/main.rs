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
//!
//! ## `cargo xtask assets icon-png <path> [--size N]`
//!
//! Renders the placeholder app icon (used by the release installers
//! before someone designs a real brand mark) to a PNG. See
//! `assets.rs` for the rationale on why this is procedural.

#![allow(clippy::unwrap_used, clippy::expect_used)] // build/dev tool

mod assets;
mod hunspell;

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

const EN_URL: &str = "https://raw.githubusercontent.com/dwyl/english-words/master/words_alpha.txt";
const UK_README_URL: &str =
    "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/uk_UA/README_uk_UA.txt";

// Hunspell-derived sources for the bundled languages. We download the
// `.dic` (word stems with flags) AND the matching `.aff` (affix
// rules), then run them through `hunspell::Aff::expand` to get full
// inflected surface forms. Without this, ~70 % of common verb /
// declension forms are missing — see DECISIONS.md (2026-05-07) and
// `xtask/src/hunspell.rs` for the full story.
//
// URLs spelled out in full instead of concatenated from a base —
// `concat!` only takes literals and a const-fn helper would obscure
// what's a plain list of file paths.
const UK_DIC_URL: &str =
    "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/uk_UA/uk_UA.dic";
const UK_AFF_URL: &str =
    "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/uk_UA/uk_UA.aff";
const RU_DIC_URL: &str =
    "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/ru_RU/ru_RU.dic";
const RU_AFF_URL: &str =
    "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/ru_RU/ru_RU.aff";
const DE_DIC_URL: &str =
    "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/de/de_DE_frami.dic";
const DE_AFF_URL: &str =
    "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/de/de_DE_frami.aff";
const ES_DIC_URL: &str =
    "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/es/es_ES.dic";
const ES_AFF_URL: &str =
    "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/es/es_ES.aff";
const FR_DIC_URL: &str =
    "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/fr_FR/fr.dic";
const FR_AFF_URL: &str =
    "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/fr_FR/fr.aff";

fn main() -> Result<()> {
    let rest: Vec<String> = std::env::args().skip(1).collect();
    match (
        rest.first().map(String::as_str),
        rest.get(1).map(String::as_str),
    ) {
        (Some("help") | None, _) => {
            print_help();
            Ok(())
        }
        (Some("wordlists"), Some("fetch")) => fetch_wordlists(),
        (Some("hooks"), Some("install")) => install_hooks(),
        (Some("hooks"), Some("uninstall")) => uninstall_hooks(),
        (Some("assets"), Some("icon-png")) => render_icon_command(&rest[2..]),
        (Some(other), _) => bail!("unknown xtask command: {other} (try `cargo xtask help`)"),
    }
}

fn print_help() {
    println!("xtask commands:");
    println!("  help                  Show this list.");
    println!("  wordlists fetch       Re-download and re-process the embedded dictionaries.");
    println!("  hooks install         Wire `.githooks/` into this clone (sets core.hooksPath).");
    println!("  hooks uninstall       Unset core.hooksPath (revert to default `.git/hooks/`).");
    println!("  assets icon-png <out> [--size N]");
    println!(
        "                         Render the placeholder app icon as a PNG (default size 1024)."
    );
}

/// Parse `<out-path> [--size N]` and render the icon.
///
/// Tiny ad-hoc parser instead of a clap dep — we only have one flag,
/// and the xtask crate has been resolutely zero-config so far.
fn render_icon_command(args: &[String]) -> Result<()> {
    let mut out_path: Option<PathBuf> = None;
    let mut size: u32 = 1024;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--size" => {
                let v = args
                    .get(i + 1)
                    .context("--size needs a value (e.g. --size 256)")?;
                size = v
                    .parse()
                    .with_context(|| format!("--size {v}: not a u32"))?;
                i += 2;
            }
            other if !other.starts_with('-') && out_path.is_none() => {
                out_path = Some(PathBuf::from(other));
                i += 1;
            }
            other => bail!(
                "unexpected argument {other:?} (usage: cargo xtask assets icon-png <out-path> [--size N])"
            ),
        }
    }
    let out = out_path.context("missing output path (cargo xtask assets icon-png <out>)")?;
    assets::render_app_icon(size, &out)?;
    println!("rendered {}×{} icon to {}", size, size, out.display());
    Ok(())
}

fn fetch_wordlists() -> Result<()> {
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
fn fetch_hunspell(
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

fn download(url: &str, dest: &Path) -> Result<()> {
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
fn read_hunspell_text(path: &Path) -> Result<String> {
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
fn process_hunspell_with_aff(dic: &Path, aff: &Path, output: &Path) -> Result<()> {
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
fn write_sorted(path: &Path, words: &BTreeSet<String>) -> Result<()> {
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
