//! On-disk reading: wordlist files, dictionary assembly, user
//! override directories.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use fst::Set as FstSet;
use poltertype_detect::LayoutDictionary;
use tracing::{info, warn};

/// Parse a one-word-per-line file into a `HashSet` normalised exactly
/// as a typed token is by the dictionary detector — non-letters
/// stripped, lowercased.
///
/// Without that, the on-disk format and the lookup pipeline disagree:
/// `letters_only_lower` strips hyphens, apostrophes and digits off the
/// buffered token, so an entry like `v-strel-zbook` or `ім'я` stored
/// verbatim never matched the lookup key.
///
/// Blank lines and `#` comments are skipped, as are lines with no
/// alphabetic characters at all — they would normalise to the empty
/// string and pollute the set.
pub fn parse_wordlist(input: &str) -> HashSet<String> {
    input
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(poltertype_detect::letters_only_lower)
        .filter(|w| !w.is_empty())
        .collect()
}

/// Construct a [`LayoutDictionary`] for `stem` from the bundled FST and
/// stop-word file in `<data_dir>/wordlists/`.
///
/// `overlay_dir` can extend either layer at runtime:
///
/// | User file | Merged into | Use case |
/// |---|---|---|
/// | `<stem>.txt` | `user_overlay` | runtime additions |
/// | `<stem>-extras.txt` | `user_overlay` | same, separate file for organisation |
/// | `<stem>-stop.txt` | `short_stop_words` | extend the ≤2-letter list |
/// | `<stem>-weak.txt` | `weak` | Hunspell-valid but rare, so a strong cross-layout hit wins |
///
/// Missing files are silently fine; read errors are logged and the
/// bundled data keeps working.
pub fn build_dictionary(
    data_dir: &Path,
    stem: &str,
    overlay_dir: Option<&Path>,
) -> Option<LayoutDictionary> {
    let fst_path = data_dir.join("wordlists").join(format!("{stem}.fst"));
    let bytes = match std::fs::read(&fst_path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            warn!(
                ?fst_path,
                stem, "FST not found; skipping dict for this layout"
            );
            return None;
        }
        Err(e) => {
            tracing::error!(?fst_path, stem, err = %e, "FST read failed; skipping dict");
            return None;
        }
    };
    // FST bytes outlive every reasonable use of the dictionary —
    // they're loaded once on startup and the dictionary handles
    // get cloned into detectors that live for the whole program.
    // Leaking matches the previous `include_bytes!` lifetime
    // exactly (`&'static [u8]`) at the cost of one allocation per
    // language at startup.
    let leaked: &'static [u8] = Box::leak(bytes.into_boxed_slice());
    let bundled_fst = match FstSet::new(leaked) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(stem, err = %e, "bundled FST is malformed; skipping dict for this layout");
            return None;
        }
    };

    // ── short-stop list: bundled baseline + optional user file ──
    let mut short_stop_words = read_stop_words(data_dir, stem);
    if let Some(extra) = load_overlay_file(overlay_dir, stem, "-stop") {
        short_stop_words.extend(extra);
    }

    // ── user overlay: <stem>.txt and <stem>-extras.txt merged ──
    let mut user_overlay: HashSet<String> = HashSet::new();
    if let Some(extra) = load_overlay_file(overlay_dir, stem, "") {
        user_overlay.extend(extra);
    }
    if let Some(extra) = load_overlay_file(overlay_dir, stem, "-extras") {
        user_overlay.extend(extra);
    }

    // ── weak list: bundled baseline + optional user file ──
    let mut weak = read_weak_words(data_dir, stem);
    if let Some(extra) = load_overlay_file(overlay_dir, stem, "-weak") {
        weak.extend(extra);
    }

    let mut dict = LayoutDictionary::new(bundled_fst, user_overlay, short_stop_words, weak);

    // ── surface-form FST (suggestions corpus) ──
    // Same leak-for-'static pattern as the membership FST above.
    // Missing file is fine: older data dirs predate suggestions, and
    // the feature degrades to overlay-only candidates.
    let surface_path = data_dir
        .join("wordlists")
        .join(format!("{stem}-surface.fst"));
    match std::fs::read(&surface_path) {
        Ok(bytes) => {
            let leaked: &'static [u8] = Box::leak(bytes.into_boxed_slice());
            match FstSet::new(leaked) {
                Ok(s) => dict = dict.with_surface(s),
                Err(e) => {
                    tracing::error!(stem, err = %e, "surface FST is malformed; suggestions degraded for this layout");
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            info!(stem, "no surface FST; suggestions degraded for this layout");
        }
        Err(e) => {
            warn!(?surface_path, err = %e, "surface FST read failed; suggestions degraded");
        }
    }

    Some(dict)
}

/// Read `<data_dir>/wordlists/<stem>-stop.txt` if it exists. Missing
/// → empty set (treated identically by `LayoutDictionary`).
pub fn read_stop_words(data_dir: &Path, stem: &str) -> HashSet<String> {
    let path = data_dir.join("wordlists").join(format!("{stem}-stop.txt"));
    match std::fs::read_to_string(&path) {
        Ok(s) => parse_wordlist(&s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => HashSet::new(),
        Err(e) => {
            warn!(?path, err = %e, "could not read stop-words file");
            HashSet::new()
        }
    }
}

/// Read `<data_dir>/wordlists/<stem>-weak.txt` if it exists. Same
/// shape as the stop / extras readers — one canonicalised word per
/// line, missing file is silently fine. See [`LayoutDictionary::weak`]
/// for the semantics.
pub fn read_weak_words(data_dir: &Path, stem: &str) -> HashSet<String> {
    let path = data_dir.join("wordlists").join(format!("{stem}-weak.txt"));
    match std::fs::read_to_string(&path) {
        Ok(s) => parse_wordlist(&s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => HashSet::new(),
        Err(e) => {
            warn!(?path, err = %e, "could not read weak-words file");
            HashSet::new()
        }
    }
}

/// User-layout dictionary builder: like [`build_dictionary`] but
/// without a bundled FST (user layouts can't ship binary blobs). The
/// dictionary is purely the overlay files in `overlay_dir`.
pub fn build_user_dictionary(stem: &str, overlay_dir: Option<&Path>) -> Option<LayoutDictionary> {
    let mut user_overlay: HashSet<String> = HashSet::new();
    let mut short_stop_words: HashSet<String> = HashSet::new();
    let mut weak: HashSet<String> = HashSet::new();
    let mut any = false;
    if let Some(extra) = load_overlay_file(overlay_dir, stem, "") {
        user_overlay.extend(extra);
        any = true;
    }
    if let Some(extra) = load_overlay_file(overlay_dir, stem, "-extras") {
        user_overlay.extend(extra);
        any = true;
    }
    if let Some(extra) = load_overlay_file(overlay_dir, stem, "-stop") {
        short_stop_words.extend(extra);
        any = true;
    }
    if let Some(extra) = load_overlay_file(overlay_dir, stem, "-weak") {
        weak.extend(extra);
        any = true;
    }
    if !any {
        return None;
    }
    Some(LayoutDictionary::from_overlay_only(
        user_overlay,
        short_stop_words,
        weak,
    ))
}

/// Read `<dir>/<stem><suffix>.txt` if present and parse it. Returns
/// `None` for both "no overlay dir configured" and "file does not
/// exist" — the caller treats both as "no user additions". Other I/O
/// errors are logged and treated the same way so a transient read
/// problem doesn't take the dict offline.
pub fn load_overlay_file(
    overlay_dir: Option<&Path>,
    stem: &str,
    suffix: &str,
) -> Option<HashSet<String>> {
    let dir = overlay_dir?;
    let path = dir.join(format!("{stem}{suffix}.txt"));
    match std::fs::read_to_string(&path) {
        Ok(s) => {
            let parsed = parse_wordlist(&s);
            info!(
                ?path,
                lines = s.lines().count(),
                words = parsed.len(),
                "merged user wordlist override"
            );
            Some(parsed)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            warn!(?path, err = %e, "could not read user wordlist override");
            None
        }
    }
}

/// Path under which user-supplied wordlist overrides live:
/// `<config-dir>/poltertype/wordlists/`. Three optional files per
/// layout (see [`build_dictionary`]).
pub fn user_wordlist_dir() -> Option<PathBuf> {
    crate::settings::SettingsStore::project_dirs()
        .ok()
        .map(|dirs| dirs.config_dir().join("wordlists"))
}

/// Path to the per-profile wordlist directory:
/// `<config-dir>/poltertype/wordlists/profiles/<profile-id>/`.
/// The id is taken verbatim — the caller is responsible for
/// rejecting path-unsafe ids before letting them reach this
/// function (see `wordlist_profiles::WordlistProfile::id` docs
/// for the expected shape).
pub fn user_profile_wordlist_dir(profile_id: &str) -> Option<PathBuf> {
    user_wordlist_dir().map(|d| d.join("profiles").join(profile_id))
}

/// Path under which user-supplied **layout mapping** TOML files live:
/// `<config-dir>/poltertype/layouts/`. Any `*.toml` here is loaded
/// alongside the bundled layouts.
pub fn user_layout_dir() -> Option<PathBuf> {
    crate::settings::SettingsStore::project_dirs()
        .ok()
        .map(|dirs| dirs.config_dir().join("layouts"))
}

/// Read every `*.toml` file in `dir` and return parsed
/// `(file_stem, body)` pairs sorted by file name.
pub fn read_user_layout_files(dir: &Path) -> Vec<(String, String)> {
    let entries = match std::fs::read_dir(dir) {
        Ok(it) => it,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            warn!(?dir, err = %e, "could not enumerate user layouts dir");
            return Vec::new();
        }
    };
    let mut out: Vec<(String, String)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("toml") {
            continue;
        }
        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_owned(),
            None => continue,
        };
        match std::fs::read_to_string(&path) {
            Ok(body) => out.push((stem, body)),
            Err(e) => warn!(?path, err = %e, "could not read user layout file"),
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}
