//! Layout mappings — what each Win-SC1 scancode produces under each
//! supported layout. Loaded at runtime from the on-disk **data
//! directory** resolved by [`crate::data_dir`].
//!
//! ## On-disk layout
//!
//! ```text
//! <data_dir>/
//!   layout-mappings/<stem>.toml         ← mapping table
//!   wordlists/<stem>.fst                ← FST built from <stem>.txt.gz
//!   wordlists/<stem>-stop.txt           ← curated 1- / 2-letter words
//! ```
//!
//! `build.rs` writes this tree to `<workspace>/target/dist/data/` from
//! the committed sources under `data/`. Installers copy the same tree
//! next to the executable. At runtime we never re-derive an FST — we
//! just `mmap` the prepared `.fst` files.
//!
//! ## Active-layout filter
//!
//! [`LayoutDb::load`] takes an optional **active filter** — typically
//! the list returned by `LayoutSwitcher::list_active()`. When set, only
//! layouts whose `id` matches are read into memory; the others stay on
//! disk. A user with `en-US / uk-UA / ru-RU` enabled in the OS skips
//! loading ~7-15 MB of fr-FR / es-ES / de-DE FST data they'd never
//! query.
//!
//! ## User extensions
//!
//! Two override paths layered on top of the bundled set:
//!
//! 1. `<config-dir>/kb-switcher/layouts/*.toml` — add new layouts
//!    without rebuilding. Same TOML schema as the bundled ones.
//! 2. `<config-dir>/kb-switcher/wordlists/<stem>(-extras|-stop).txt`
//!    — extend the dictionary or short-stop list of any layout
//!    (bundled or user) at runtime.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use fst::Set as FstSet;
use kb_detect::{LayoutDictionary, LayoutProfile, Script};
use kb_types::{LayoutId, WordKey};
use serde::Deserialize;
use thiserror::Error;
use tracing::{info, warn};

#[derive(Debug, Error)]
pub enum LayoutLoadError {
    #[error("invalid layout TOML: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("data directory not resolved: {0}")]
    DataDir(#[from] crate::data_dir::DataDirError),
}

/// Bundled layout stems shipped in the data directory. Used as the
/// default discovery list when scanning `<data_dir>/layout-mappings/`
/// — we don't enumerate that directory at runtime because:
///
/// * The bundled set is curated (mappings reviewed against OS docs)
///   while user-side TOMLs are accepted on best-effort.
/// * Filesystem enumeration is one more failure mode (permission,
///   non-UTF-8 names) we don't need on the hot startup path.
///
/// Kept in lock-step with `build.rs::LAYOUTS`. A mismatch shows up
/// as a "missing TOML" warning at startup, never silently.
const BUNDLED_LAYOUT_STEMS: &[&str] = &["en_us", "uk_ua", "ru_ru", "de_de", "es_es", "fr_fr"];

// ─── Raw TOML schema ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct LayoutToml {
    id: String,
    #[serde(default)]
    name: Option<String>,
    script: Script,
    keys: HashMap<String, KeyMapping>,
}

#[derive(Debug, Deserialize)]
struct KeyMapping {
    plain: String,
    #[serde(default)]
    shift: Option<String>,
}

// ─── In-memory representation ────────────────────────────────────────

#[derive(Clone)]
pub struct LayoutMapping {
    pub id: LayoutId,
    pub name: String,
    pub script: Script,
    /// scancode → (unshifted, shifted) characters.
    pub keys: HashMap<u32, (char, Option<char>)>,
    /// Pre-computed lower-case vowel set, for the detector profile.
    pub vowels: Vec<char>,
    /// Per-layout dictionary (compact FST + optional user-overlay).
    /// `None` means we have no dictionary for this layout — the
    /// detector falls through to plausibility-only.
    pub dictionary: Option<LayoutDictionary>,
}

impl std::fmt::Debug for LayoutMapping {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LayoutMapping")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("script", &self.script)
            .field("keys_n", &self.keys.len())
            .field("dictionary_present", &self.dictionary.is_some())
            .finish()
    }
}

impl LayoutMapping {
    pub fn from_toml_str(input: &str) -> Result<Self, LayoutLoadError> {
        Self::from_parts(input, None)
    }

    /// Parse the layout TOML; bind an optional dictionary (FST blob +
    /// user overlay) to the resulting mapping.
    pub fn from_parts(
        toml_input: &str,
        dictionary: Option<LayoutDictionary>,
    ) -> Result<Self, LayoutLoadError> {
        let raw: LayoutToml = toml::from_str(toml_input)?;
        let id = LayoutId::new(raw.id);
        let name = raw.name.unwrap_or_else(|| id.as_str().to_owned());
        let script = raw.script;

        let mut keys = HashMap::with_capacity(raw.keys.len());
        for (k, v) in raw.keys {
            let Some(scancode) = parse_scancode(&k) else {
                warn!(key = %k, layout = %id, "ignoring unparseable scancode");
                continue;
            };
            let plain = first_char(&v.plain);
            let shift = v.shift.as_deref().and_then(first_char);
            if let Some(plain) = plain {
                keys.insert(scancode, (plain, shift));
            }
        }

        let vowels = derive_vowels(&id, script);

        Ok(Self {
            id,
            name,
            script,
            keys,
            vowels,
            dictionary,
        })
    }

    /// Translate a single keystroke to the character produced under
    /// this layout, if known.
    pub fn translate_key(&self, key: WordKey) -> Option<char> {
        let (plain, shifted) = self.keys.get(&key.scancode)?;
        if key.shift {
            Some(shifted.unwrap_or(*plain))
        } else {
            Some(*plain)
        }
    }

    /// Translate an entire word buffer into a string under this layout.
    /// Untranslatable keystrokes are dropped silently.
    pub fn translate_buffer(&self, keys: &[WordKey]) -> String {
        let mut out = String::with_capacity(keys.len());
        for &k in keys {
            if let Some(c) = self.translate_key(k) {
                out.push(c);
            }
        }
        out
    }

    /// Build the [`LayoutProfile`] used by the detector pipeline.
    pub fn detector_profile(&self) -> LayoutProfile {
        LayoutProfile::new(self.id.clone(), self.script, self.vowels.iter().copied())
    }
}

fn parse_scancode(s: &str) -> Option<u32> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(rest, 16).ok()
    } else {
        s.parse::<u32>().ok()
    }
}

/// Parse a one-word-per-line text file into a HashSet whose entries
/// are normalized exactly like a typed token passes through the
/// dictionary detector — non-letter chars stripped, lowercase.
///
/// Without this normalization, the on-disk format and the lookup
/// pipeline disagreed: `kb_detect::letters_only_lower` strips
/// hyphens / apostrophes / digits off the buffered token before
/// hitting the overlay HashSet, so an extras line like
/// `v-strel-zbook` (a hostname the user wants to type as English)
/// or `ім'я` was stored verbatim and never matched the canonicalised
/// lookup key (`vstrelzbook`, `імя`).
///
/// Blank lines and `#` comments are skipped. Lines that contain no
/// alphabetic chars at all (`---`, `42`) are dropped — they'd
/// normalize to the empty string and pollute the set.
fn parse_wordlist(input: &str) -> HashSet<String> {
    input
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(kb_detect::letters_only_lower)
        .filter(|w| !w.is_empty())
        .collect()
}

/// Construct a [`LayoutDictionary`] for `stem`, reading the bundled
/// FST and stop-word file from `<data_dir>/wordlists/`.
///
/// `overlay_dir` (typically `<config-dir>/kb-switcher/wordlists/`)
/// can extend either layer at runtime:
///
/// | User file                | Merged into        | Use case |
/// |--------------------------|--------------------|----------|
/// | `<stem>.txt`             | `user_overlay`     | runtime additions |
/// | `<stem>-extras.txt`      | `user_overlay`     | same; separate file for organisation |
/// | `<stem>-stop.txt`        | `short_stop_words` | extend the ≤2-letter list |
/// | `<stem>-weak.txt`        | `weak`             | mark Hunspell-valid-but-rare entries (e.g. archaic vocatives) so a strong cross-layout dict hit wins |
///
/// Missing files are silently fine. Read errors are logged and the
/// bundled data continues to work.
fn build_dictionary(
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

    Some(LayoutDictionary::new(
        bundled_fst,
        user_overlay,
        short_stop_words,
        weak,
    ))
}

/// Read `<data_dir>/wordlists/<stem>-stop.txt` if it exists. Missing
/// → empty set (treated identically by `LayoutDictionary`).
fn read_stop_words(data_dir: &Path, stem: &str) -> HashSet<String> {
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
fn read_weak_words(data_dir: &Path, stem: &str) -> HashSet<String> {
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
fn build_user_dictionary(stem: &str, overlay_dir: Option<&Path>) -> Option<LayoutDictionary> {
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
fn load_overlay_file(
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
/// `<config-dir>/kb-switcher/wordlists/`. Three optional files per
/// layout (see [`build_dictionary`]).
pub fn user_wordlist_dir() -> Option<PathBuf> {
    crate::settings::SettingsStore::project_dirs()
        .ok()
        .map(|dirs| dirs.config_dir().join("wordlists"))
}

/// Path to the per-profile wordlist directory:
/// `<config-dir>/kb-switcher/wordlists/profiles/<profile-id>/`.
/// The id is taken verbatim — the caller is responsible for
/// rejecting path-unsafe ids before letting them reach this
/// function (see `wordlist_profiles::WordlistProfile::id` docs
/// for the expected shape).
pub fn user_profile_wordlist_dir(profile_id: &str) -> Option<PathBuf> {
    user_wordlist_dir().map(|d| d.join("profiles").join(profile_id))
}

/// Path under which user-supplied **layout mapping** TOML files live:
/// `<config-dir>/kb-switcher/layouts/`. Any `*.toml` here is loaded
/// alongside the bundled layouts.
pub fn user_layout_dir() -> Option<PathBuf> {
    crate::settings::SettingsStore::project_dirs()
        .ok()
        .map(|dirs| dirs.config_dir().join("layouts"))
}

/// Read every `*.toml` file in `dir` and return parsed
/// `(file_stem, body)` pairs sorted by file name.
fn read_user_layout_files(dir: &Path) -> Vec<(String, String)> {
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

fn first_char(s: &str) -> Option<char> {
    s.chars().next()
}

// ─── Plug-in packs ──────────────────────────────────────────────────

/// Manifest schema for a plug-in language pack. Lives at
/// `<data_dir>/plugins/<dir-name>/manifest.toml`. Mirrors the contract
/// documented in `docs/DATA_LAYOUT.md`.
///
/// Forward-compat: every field is `#[serde(default)]` so future
/// additions (signing, capability flags) won't break older app
/// versions; old fields stay parseable as the schema grows.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct PluginManifest {
    /// Pack identifier (NOT a layout id — it's the pack's own name,
    /// for log messages and conflict diagnostics).
    id: String,
    /// Human-readable display name.
    name: String,
    /// Pack version string. Free-form; we don't enforce semver yet.
    version: String,
    /// Optional self-declared layouts. Currently only used for log
    /// output; the loader actually enumerates the pack's
    /// `layout-mappings/` directory regardless.
    #[serde(default)]
    supported_layouts: Vec<String>,
}

/// Enumerate `<data_dir>/plugins/*/` and merge each pack's layouts
/// into `by_id`. Loud-but-graceful at every level — a single broken
/// pack never takes down the rest of the load.
///
/// **v1 surface is data-only.** A pack ships:
///
///   * `manifest.toml` — required; missing → skip pack.
///   * `layout-mappings/*.toml` — keyboard maps; same TOML schema as
///     bundled / user layouts.
///   * `wordlists/<stem>.fst` + optional `<stem>-stop.txt` — same
///     shape as the bundled `<data_dir>/wordlists/`.
///
/// Native code, network calls, and settings injection are explicitly
/// out of scope (see `docs/DATA_LAYOUT.md` § "What plug-ins won't be").
/// Until those land, the loader can stay this small and reviewable.
fn load_plugin_packs(
    data_dir: &Path,
    user_wordlist_dir: Option<&Path>,
    by_id: &mut HashMap<LayoutId, LayoutMapping>,
) {
    let plugins_dir = data_dir.join("plugins");
    let entries = match std::fs::read_dir(&plugins_dir) {
        Ok(it) => it,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => {
            warn!(?plugins_dir, err = %e, "could not enumerate plugins dir");
            return;
        }
    };

    // Sort for deterministic load order — two packs claiming the
    // same layout id should resolve in alphabetical order, not
    // whatever the filesystem felt like today.
    let mut pack_dirs: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.path())
        .collect();
    pack_dirs.sort();

    for pack_dir in pack_dirs {
        load_one_pack(&pack_dir, user_wordlist_dir, by_id);
    }
}

fn load_one_pack(
    pack_dir: &Path,
    user_wordlist_dir: Option<&Path>,
    by_id: &mut HashMap<LayoutId, LayoutMapping>,
) {
    let manifest_path = pack_dir.join("manifest.toml");
    let manifest_text = match std::fs::read_to_string(&manifest_path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            warn!(
                ?pack_dir,
                "plug-in directory missing manifest.toml — skipping"
            );
            return;
        }
        Err(e) => {
            warn!(?manifest_path, err = %e, "could not read plug-in manifest");
            return;
        }
    };
    let manifest: PluginManifest = match toml::from_str(&manifest_text) {
        Ok(m) => m,
        Err(e) => {
            warn!(?manifest_path, err = %e, "invalid plug-in manifest TOML; skipping");
            return;
        }
    };
    info!(
        pack_id = %manifest.id,
        name = %manifest.name,
        version = %manifest.version,
        supported = ?manifest.supported_layouts,
        ?pack_dir,
        "loading plug-in pack"
    );

    let layouts_dir = pack_dir.join("layout-mappings");
    let entries = match std::fs::read_dir(&layouts_dir) {
        Ok(it) => it,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            warn!(
                ?layouts_dir,
                "plug-in pack has no layout-mappings/ directory"
            );
            return;
        }
        Err(e) => {
            warn!(?layouts_dir, err = %e, "could not enumerate plug-in layouts");
            return;
        }
    };

    for entry in entries.flatten() {
        let toml_path = entry.path();
        if toml_path.extension().and_then(|s| s.to_str()) != Some("toml") {
            continue;
        }
        let stem = match toml_path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_owned(),
            None => continue,
        };
        let body = match std::fs::read_to_string(&toml_path) {
            Ok(s) => s,
            Err(e) => {
                warn!(?toml_path, err = %e, "could not read plug-in TOML");
                continue;
            }
        };

        // Plug-in's own FST + optional stop-word txt sit alongside
        // its TOML, in the pack's `wordlists/` directory. We reuse
        // `build_dictionary` with `pack_dir` standing in for
        // `data_dir` — the function only looks at `<data_dir>/wordlists/`,
        // so this is exactly the right shape.
        //
        // User-side wordlist overlay still applies on top, so a user
        // can extend a plug-in's vocabulary the same way they extend
        // the bundled one.
        let dictionary = build_dictionary(pack_dir, &stem, user_wordlist_dir);
        match LayoutMapping::from_parts(&body, dictionary) {
            Ok(layout) => {
                let overriding = by_id.contains_key(&layout.id);
                info!(
                    layout = %layout.id,
                    keys = layout.keys.len(),
                    dict = layout.dictionary.is_some(),
                    pack = %manifest.id,
                    overriding,
                    "loaded plug-in layout"
                );
                by_id.insert(layout.id.clone(), layout);
            }
            Err(e) => {
                warn!(?toml_path, pack = %manifest.id, err = %e, "failed to parse plug-in TOML; skipping");
            }
        }
    }
}

/// Default vowel set per script — adjusted in code for languages whose
/// vowels diverge from the script default.
fn derive_vowels(id: &LayoutId, script: Script) -> Vec<char> {
    match id.as_str() {
        "uk-UA" => "аеиіоуюяєї".chars().collect(),
        "ru-RU" => "аеёиоуыэюя".chars().collect(),
        "de-DE" => "aeiouäöü".chars().collect(),
        "es-ES" => "aeiouáéíóúü".chars().collect(),
        "fr-FR" => "aeiouyàâéèêëîïôûùüÿ".chars().collect(),
        _ => match script {
            Script::Latin => "aeiouy".chars().collect(),
            Script::Cyrillic => "аеиіоуюяєїыэё".chars().collect(),
            Script::Greek => "αεηιουω".chars().collect(),
            Script::Armenian => "աեէիոույ".chars().collect(),
            Script::Hebrew | Script::Arabic | Script::Other => Vec::new(),
        },
    }
}

// ─── LayoutDb: handle to all loaded layouts ──────────────────────────

/// Configuration for [`LayoutDb::load`]. Pulled out as a struct rather
/// than a tower of optional args because the call site grew to 4
/// orthogonal flags and positional `Option<&Path>` quickly gets
/// unreadable.
#[derive(Debug, Clone, Default)]
pub struct LoadOptions<'a> {
    /// Directory holding the bundled `layout-mappings/` and
    /// `wordlists/` trees. `None` → resolve via [`crate::data_dir`].
    pub data_dir: Option<&'a Path>,
    /// If `Some(list)`, only load layouts whose id is in `list`.
    /// `None` loads every bundled stem (used by tests and as a
    /// fail-open fallback when the OS query for active layouts
    /// fails).
    pub active_filter: Option<&'a [LayoutId]>,
    /// `<config-dir>/kb-switcher/layouts/` — user-supplied TOMLs.
    pub user_layout_dir: Option<&'a Path>,
    /// `<config-dir>/kb-switcher/wordlists/` — user wordlist overlays.
    pub user_wordlist_dir: Option<&'a Path>,
}

#[derive(Debug, Clone, Default)]
pub struct LayoutDb {
    by_id: HashMap<LayoutId, LayoutMapping>,
    /// `(scancode, shift_state)` pairs that map to an alphabetic
    /// character in *at least one* loaded layout. Used by
    /// [`WordBuffer`] to keep Cyrillic words intact when the user is
    /// typing them while the en-US layout is active (and vice-versa).
    /// Precomputed on load — cheap to query per-keystroke.
    letter_scancodes: HashSet<(u32, bool)>,
}

impl LayoutDb {
    /// Top-level loader. Resolves the data directory if not supplied,
    /// scans bundled stems and (optionally) user-side TOMLs, and
    /// returns a populated `LayoutDb`. Filter rules:
    ///
    /// * Bundled layouts whose id is **not** in `active_filter` (when
    ///   set) are skipped — their FSTs never enter memory.
    /// * User layouts always load (the user dropping a TOML in
    ///   `<config-dir>/layouts/` is an explicit "I want this").
    pub fn load(opts: LoadOptions<'_>) -> Result<Self, LayoutLoadError> {
        let resolved_data_dir;
        let data_dir: &Path = match opts.data_dir {
            Some(p) => p,
            None => {
                resolved_data_dir = crate::data_dir::resolve()?;
                resolved_data_dir.as_path()
            }
        };

        let mut by_id = HashMap::new();

        // ── Bundled stems ─────────────────────────────────────────
        for stem in BUNDLED_LAYOUT_STEMS {
            let toml_path = data_dir
                .join("layout-mappings")
                .join(format!("{stem}.toml"));
            let body = match std::fs::read_to_string(&toml_path) {
                Ok(s) => s,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    warn!(?toml_path, stem, "bundled layout TOML missing; skipping");
                    continue;
                }
                Err(e) => {
                    tracing::error!(?toml_path, stem, err = %e, "could not read bundled layout TOML");
                    continue;
                }
            };

            // Pre-parse just enough to know the BCP-47 id, so we can
            // filter against `active_filter` BEFORE doing the
            // expensive FST mmap+parse for layouts the user can't
            // even reach.
            let raw_id = match peek_layout_id(&body) {
                Some(id) => LayoutId::new(id),
                None => {
                    warn!(?toml_path, "TOML has no `id` field; skipping");
                    continue;
                }
            };
            if let Some(filter) = opts.active_filter {
                if !filter.contains(&raw_id) {
                    info!(layout = %raw_id, "skipping bundled layout — not in active OS list");
                    continue;
                }
            }

            let dictionary = build_dictionary(data_dir, stem, opts.user_wordlist_dir);
            match LayoutMapping::from_parts(&body, dictionary) {
                Ok(layout) => {
                    info!(
                        layout = %layout.id,
                        keys = layout.keys.len(),
                        dict = layout.dictionary.is_some(),
                        "loaded bundled layout"
                    );
                    by_id.insert(layout.id.clone(), layout);
                }
                Err(e) => {
                    tracing::error!(?toml_path, err = %e, "failed to parse bundled layout");
                }
            }
        }

        // ── Plug-in packs (read-only data drops next to bundled
        //                  set — see docs/DATA_LAYOUT.md) ───────────
        //
        // Loaded BEFORE user-side TOMLs so the precedence chain is:
        //   bundled  ←  plug-ins  ←  user-overlay
        // i.e. a user can still override a plug-in by dropping a TOML
        // with the same id under `<config-dir>/kb-switcher/layouts/`.
        load_plugin_packs(data_dir, opts.user_wordlist_dir, &mut by_id);

        // ── User-side TOMLs (always loaded, never filtered) ───────
        if let Some(dir) = opts.user_layout_dir {
            for (stem, body) in read_user_layout_files(dir) {
                let dictionary = build_user_dictionary(&stem, opts.user_wordlist_dir);
                match LayoutMapping::from_parts(&body, dictionary) {
                    Ok(layout) => {
                        let overriding = by_id.contains_key(&layout.id);
                        info!(
                            layout = %layout.id,
                            keys = layout.keys.len(),
                            dict = layout.dictionary.is_some(),
                            stem,
                            overriding,
                            "loaded user layout"
                        );
                        by_id.insert(layout.id.clone(), layout);
                    }
                    Err(e) => {
                        warn!(stem, err = %e, "failed to parse user layout TOML; skipping");
                    }
                }
            }
        }

        let letter_scancodes = compute_letter_scancodes(&by_id);
        Ok(Self {
            by_id,
            letter_scancodes,
        })
    }

    /// Convenience: load every bundled layout from the auto-resolved
    /// data dir, with no user overlay and no active filter. Mainly
    /// for tests and tooling — production code uses [`Self::load`]
    /// with the OS active-layouts list.
    ///
    /// Panics on data-dir resolution failure: if the convenience
    /// path can't find data, there's nothing useful to do without
    /// it. Production code that wants to recover should call
    /// [`Self::load`] directly and handle the `Result`.
    #[allow(clippy::panic, clippy::missing_panics_doc)]
    pub fn load_embedded() -> Self {
        match Self::load(LoadOptions::default()) {
            Ok(db) => db,
            Err(e) => panic!("LayoutDb::load_embedded: {e}"),
        }
    }

    /// Like [`Self::load_embedded`] but lets the caller hand in a
    /// user-overlay directory. Same panic-on-failure contract.
    #[allow(clippy::panic, clippy::missing_panics_doc)]
    pub fn load_embedded_with_user_overlay(overlay_dir: Option<&Path>) -> Self {
        match Self::load(LoadOptions {
            user_wordlist_dir: overlay_dir,
            ..Default::default()
        }) {
            Ok(db) => db,
            Err(e) => panic!("LayoutDb::load_embedded_with_user_overlay: {e}"),
        }
    }

    /// Like [`Self::load`] but with bundled + user layouts only —
    /// kept for the existing tests that drive the user-layout
    /// pipeline. Same panic-on-failure contract.
    #[allow(clippy::panic, clippy::missing_panics_doc)]
    pub fn load_with_user_layouts(layout_dir: Option<&Path>, overlay_dir: Option<&Path>) -> Self {
        match Self::load(LoadOptions {
            user_layout_dir: layout_dir,
            user_wordlist_dir: overlay_dir,
            ..Default::default()
        }) {
            Ok(db) => db,
            Err(e) => panic!("LayoutDb::load_with_user_layouts: {e}"),
        }
    }

    pub fn get(&self, id: &LayoutId) -> Option<&LayoutMapping> {
        self.by_id.get(id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &LayoutId> {
        self.by_id.keys()
    }

    /// Build a profile-specific dictionary set — the same shape
    /// `DictionaryDetector::replace_dicts` accepts — using the
    /// overlay files in `profile_overlay_dir` instead of the
    /// global one.
    ///
    /// Each entry uses the layout's existing bundled FST (cheap —
    /// `LayoutDictionary` already shares the FST through `Arc`) but
    /// reads the user-overlay text files from the profile directory.
    /// The result is callers can build a per-profile dictionary set
    /// at startup, cache it, and atomically swap on focus change
    /// without rebuilding any FSTs.
    ///
    /// Layouts that have no FST in the data dir (user-supplied
    /// layouts that came in via `<config-dir>/layouts/`) get an
    /// overlay-only dictionary if any of the profile's overlay
    /// files mention them, or are skipped if not.
    ///
    /// ## Why not `&self`-only
    ///
    /// We need the data dir for the FST path; resolving it
    /// internally would lose the explicit-config-dir test path. The
    /// caller already has the resolved dir from `LoadOptions` so we
    /// take it as a parameter — same convention as the rest of
    /// this module's public API.
    pub fn build_profile_dictionaries(
        &self,
        data_dir: &Path,
        profile_overlay_dir: &Path,
    ) -> HashMap<LayoutId, kb_detect::LayoutDictionary> {
        let mut out = HashMap::new();
        for (id, mapping) in &self.by_id {
            // Stem inference for bundled layouts is "lowercase the
            // BCP-47 id, replace `-` with `_`" — same convention the
            // bundled FST file names follow. For user layouts the
            // stem is whatever the TOML's filename was, which we
            // don't track here; fall back to the same lowercased-id
            // shape so user layouts that follow the convention
            // (most do) pick up profile overlays automatically.
            let stem = mapping.id.as_str().to_lowercase().replace('-', "_");
            if let Some(dict) = build_dictionary(data_dir, &stem, Some(profile_overlay_dir)) {
                out.insert(id.clone(), dict);
            } else {
                // No bundled FST AND no profile overlay file. Try
                // overlay-only — useful for user layouts that don't
                // ship an FST but whose users still want to add
                // profile-specific words.
                if let Some(dict) = build_user_dictionary(&stem, Some(profile_overlay_dir)) {
                    out.insert(id.clone(), dict);
                }
            }
        }
        out
    }

    pub fn iter(&self) -> impl Iterator<Item = (&LayoutId, &LayoutMapping)> {
        self.by_id.iter()
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// Does this `(scancode, shift)` keystroke produce an alphabetic
    /// character in *any* of the loaded layouts?
    pub fn is_letter_in_any_layout(&self, scancode: u32, shift: bool) -> bool {
        self.letter_scancodes.contains(&(scancode, shift))
    }
}

/// Cheap pre-parse: extract the `id = "..."` line from a layout TOML
/// without paying for the full `toml::from_str`. Returns `None` if
/// the file is malformed in a way that obviously won't parse later
/// either — caller can skip noisily.
///
/// We accept either single- or double-quoted strings, optional
/// surrounding whitespace, and skip `#` comments. This is pure
/// regex-with-discipline territory; the trade-off is that a
/// pathological TOML with `id` inside a multi-line string would
/// confuse us, but every real layout TOML has the id on a top-level
/// line and we'd rather pay 50µs of grep-ish parsing than full TOML
/// parse + clone for layouts we're going to skip anyway.
fn peek_layout_id(toml: &str) -> Option<String> {
    for line in toml.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let rest = match line.strip_prefix("id") {
            Some(r) => r.trim_start(),
            None => continue,
        };
        let rest = rest.strip_prefix('=')?.trim_start();
        let (open, close) = if let Some(r) = rest.strip_prefix('"') {
            (r, '"')
        } else if let Some(r) = rest.strip_prefix('\'') {
            (r, '\'')
        } else {
            continue;
        };
        let end = open.find(close)?;
        return Some(open[..end].to_owned());
    }
    None
}

fn compute_letter_scancodes(by_id: &HashMap<LayoutId, LayoutMapping>) -> HashSet<(u32, bool)> {
    let mut out = HashSet::new();
    for mapping in by_id.values() {
        for (&sc, (plain, shift)) in &mapping.keys {
            if plain.is_alphabetic() {
                out.insert((sc, false));
            }
            if shift.is_some_and(char::is_alphabetic) {
                out.insert((sc, true));
            }
        }
    }
    out
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_layouts_load() {
        let db = LayoutDb::load_embedded();
        for id in ["en-US", "uk-UA", "ru-RU", "de-DE", "es-ES", "fr-FR"] {
            assert!(
                db.get(&LayoutId::from(id)).is_some(),
                "embedded layout `{id}` did not load"
            );
        }
    }

    /// The active-filter feature: only requested layouts enter memory.
    /// This is what saves RAM for users who don't have all six bundled
    /// languages installed in the OS.
    #[test]
    fn active_filter_drops_unrequested_layouts() {
        let want = [LayoutId::from("en-US"), LayoutId::from("uk-UA")];
        let db = LayoutDb::load(LoadOptions {
            active_filter: Some(&want),
            ..Default::default()
        })
        .expect("load with filter");
        assert!(db.get(&LayoutId::from("en-US")).is_some());
        assert!(db.get(&LayoutId::from("uk-UA")).is_some());
        // The rest are bundled-but-filtered → must NOT be in the DB.
        for skipped in ["ru-RU", "de-DE", "es-ES", "fr-FR"] {
            assert!(
                db.get(&LayoutId::from(skipped)).is_none(),
                "filter must keep `{skipped}` out of memory"
            );
        }
    }

    /// `peek_layout_id` is the fast pre-parse used by the active-
    /// filter — must round-trip every shape of `id =` line we
    /// actually emit in our TOMLs (double-quoted + spaces) plus the
    /// shapes a hand-written user TOML might use (single-quoted, no
    /// space around `=`).
    #[test]
    fn peek_layout_id_recognises_every_shape() {
        assert_eq!(
            peek_layout_id("id = \"en-US\"\nname = \"English\""),
            Some("en-US".into())
        );
        assert_eq!(peek_layout_id("id=\"uk-UA\""), Some("uk-UA".into()));
        assert_eq!(peek_layout_id("id = 'ru-RU'"), Some("ru-RU".into()));
        // Comments and blank lines must not derail the search.
        assert_eq!(
            peek_layout_id("# heading\n\nid = \"de-DE\""),
            Some("de-DE".into())
        );
        assert_eq!(peek_layout_id("name = \"only\""), None);
    }

    #[test]
    fn letter_in_any_layout_is_shift_aware() {
        let db = LayoutDb::load_embedded();
        assert!(db.is_letter_in_any_layout(0x0C, false));
        assert!(!db.is_letter_in_any_layout(0x0C, true));
    }

    #[test]
    fn new_languages_translate_distinctive_keys() {
        let db = LayoutDb::load_embedded();
        let cases = [
            ("ru-RU", 0x10u32, false, 'й'),
            ("ru-RU", 0x29, false, 'ё'),
            ("de-DE", 0x15, false, 'z'),
            ("de-DE", 0x2C, false, 'y'),
            ("de-DE", 0x1A, false, 'ü'),
            ("es-ES", 0x27, false, 'ñ'),
            ("fr-FR", 0x10, false, 'a'),
            ("fr-FR", 0x03, false, 'é'),
        ];
        for (id, sc, shift, expected) in cases {
            let mapping = db.get(&LayoutId::from(id)).unwrap_or_else(|| {
                panic!("layout `{id}` not loaded");
            });
            let got = mapping.translate_key(WordKey {
                scancode: sc,
                shift,
                timestamp_ms: 0,
            });
            assert_eq!(
                got,
                Some(expected),
                "layout {id} sc=0x{sc:X} shift={shift}: expected `{expected}` got {got:?}"
            );
        }
    }

    #[test]
    fn wordlists_loaded_with_layouts() {
        let db = LayoutDb::load_embedded();
        let en = db.get(&LayoutId::from("en-US")).expect("en-US");
        let uk = db.get(&LayoutId::from("uk-UA")).expect("uk-UA");
        let en_dict = en.dictionary.as_ref().expect("en dictionary");
        let uk_dict = uk.dictionary.as_ref().expect("uk dictionary");
        for w in ["the", "hello", "a", "i", "function", "world", "code"] {
            assert!(en_dict.contains(w), "en dict missing `{w}`");
        }
        for w in [
            "що", "мені", "цим", "а", "і", "у", "о", "є", "я", "з", "в", "й",
        ] {
            assert!(uk_dict.contains(w), "uk dict missing `{w}`");
        }
        for w in ["слово", "привіт", "робити", "знати"] {
            assert!(uk_dict.contains(w), "uk dict missing `{w}`");
        }
    }

    #[test]
    fn round_trip_hello_through_uk() {
        let db = LayoutDb::load_embedded();
        let en = db.get(&LayoutId::from("en-US")).expect("en-US");
        let uk = db.get(&LayoutId::from("uk-UA")).expect("uk-UA");
        let buf = vec![
            WordKey {
                scancode: 0x23,
                shift: false,
                timestamp_ms: 0,
            },
            WordKey {
                scancode: 0x12,
                shift: false,
                timestamp_ms: 0,
            },
            WordKey {
                scancode: 0x26,
                shift: false,
                timestamp_ms: 0,
            },
            WordKey {
                scancode: 0x26,
                shift: false,
                timestamp_ms: 0,
            },
            WordKey {
                scancode: 0x18,
                shift: false,
                timestamp_ms: 0,
            },
        ];
        assert_eq!(en.translate_buffer(&buf), "hello");
        assert_eq!(uk.translate_buffer(&buf), "руддщ");
    }

    #[test]
    fn round_trip_pryvit_through_en() {
        let db = LayoutDb::load_embedded();
        let en = db.get(&LayoutId::from("en-US")).expect("en-US");
        let uk = db.get(&LayoutId::from("uk-UA")).expect("uk-UA");
        let buf = vec![
            WordKey {
                scancode: 0x22,
                shift: false,
                timestamp_ms: 0,
            },
            WordKey {
                scancode: 0x23,
                shift: false,
                timestamp_ms: 0,
            },
            WordKey {
                scancode: 0x30,
                shift: false,
                timestamp_ms: 0,
            },
            WordKey {
                scancode: 0x20,
                shift: false,
                timestamp_ms: 0,
            },
            WordKey {
                scancode: 0x1F,
                shift: false,
                timestamp_ms: 0,
            },
            WordKey {
                scancode: 0x31,
                shift: false,
                timestamp_ms: 0,
            },
        ];
        assert_eq!(uk.translate_buffer(&buf), "привіт");
        let en_text = en.translate_buffer(&buf);
        assert!(en_text.is_ascii());
    }

    #[test]
    fn shift_picks_uppercase() {
        let db = LayoutDb::load_embedded();
        let en = db.get(&LayoutId::from("en-US")).expect("en-US");
        let buf = vec![WordKey {
            scancode: 0x23,
            shift: true,
            timestamp_ms: 0,
        }];
        assert_eq!(en.translate_buffer(&buf), "H");
    }

    // ─── User overlay loading (runtime-extensible) ───────────────────

    struct TmpDir(PathBuf);

    impl TmpDir {
        fn new(label: &str) -> Self {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let path = std::env::temp_dir().join(format!(
                "kb-switcher-test-{label}-{}-{now}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).expect("mkdir tmp");
            Self(path)
        }

        fn write(&self, name: &str, body: &str) {
            std::fs::write(self.0.join(name), body).expect("write tmp file");
        }
    }

    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn user_overlay_picks_up_extras_file() {
        let tmp = TmpDir::new("extras");
        tmp.write("uk_ua.txt", "# user adds\nфайв\n");
        tmp.write("uk_ua-extras.txt", "# more\nекстраслово\n");

        let db = LayoutDb::load_embedded_with_user_overlay(Some(&tmp.0));
        let dict = db
            .get(&LayoutId::from("uk-UA"))
            .and_then(|l| l.dictionary.as_ref())
            .expect("uk dict");

        assert!(dict.contains("файв"), "<stem>.txt entry should be in dict");
        assert!(
            dict.contains("екстраслово"),
            "<stem>-extras.txt entry should be in dict"
        );
    }

    #[test]
    fn user_overlay_normalizes_hyphens_and_apostrophes() {
        // Regression: the wordlists tab lets users add hand-picked
        // tokens to the per-layout dictionary, but hyphenated /
        // apostrophe-bearing entries used to be stored verbatim while
        // the lookup path canonicalised the typed token via
        // `letters_only_lower`. End-result: `v-strel-zbook` in the
        // extras file never matched the buffer `v-strel-zbook`
        // (lookup key `vstrelzbook`) and the engine kept switching
        // it. Lock the canonicalisation in: the entry as written and
        // the canonical key must both resolve to a Keep.
        let tmp = TmpDir::new("normalize-hyphen");
        tmp.write("en_us-extras.txt", "v-strel-zbook\n");
        tmp.write("uk_ua-extras.txt", "ім'я\n");

        let db = LayoutDb::load_embedded_with_user_overlay(Some(&tmp.0));

        let en = db
            .get(&LayoutId::from("en-US"))
            .and_then(|l| l.dictionary.as_ref())
            .expect("en dict");
        assert!(
            en.contains("vstrelzbook"),
            "hyphenated extras entry must be looked up by its \
             letters-only canonical key"
        );

        let uk = db
            .get(&LayoutId::from("uk-UA"))
            .and_then(|l| l.dictionary.as_ref())
            .expect("uk dict");
        assert!(
            uk.contains("імя"),
            "apostrophe-bearing extras entry must be looked up by \
             its letters-only canonical key"
        );
    }

    /// Regression: 2-letter acronyms in `<stem>-extras.txt` (`ai`,
    /// `ml`, `ui`, `ux`, `db`, …) used to land **only** in the
    /// embedded FST, but the runtime short-token lookup deliberately
    /// skips the FST (the bulk dict ships short-noise like `ws` /
    /// `ax` / `oe`). Result: typing `AI` while in uk-UA produced
    /// `ФШ` and neither detector had any signal to switch — the user
    /// was stuck. `build.rs` now mirrors the ≤2-letter slice of
    /// extras into the dist `<stem>-stop.txt`, so the short regime
    /// sees them.
    #[test]
    fn short_extras_are_visible_to_short_token_lookup() {
        let db = LayoutDb::load_embedded();
        let en = db
            .get(&LayoutId::from("en-US"))
            .and_then(|l| l.dictionary.as_ref())
            .expect("en dict");

        // Exhaustive sample — every one of these is a 2-letter entry
        // in `data/wordlists/en_us-extras.txt` that a developer might
        // type as a standalone token. If the build pipeline ever
        // regresses to leaving them FST-only, this test fails loud.
        for word in ["ai", "ml", "ui", "ux", "db", "qa", "cd", "ci", "md"] {
            assert!(
                en.contains_short(word),
                "en-US `{word}` should be visible to the short-token \
                 lookup (mirrored from en_us-extras.txt by build.rs)"
            );
        }

        // Sanity: the FST-only `dwyl/english-words` short noise
        // (`ws`, `ax`, `oe`) must NOT leak in. If it does the fix
        // overshot and would block legitimate Cyrillic switches.
        for noise in ["ws", "ax", "oe"] {
            assert!(
                !en.contains_short(noise),
                "en-US `{noise}` is bulk-dict short noise — must NOT \
                 be in short_stop_words"
            );
        }
    }

    /// End-to-end regression for the weak-list pipeline: the
    /// bundled uk-UA dict ships with `туче` flagged weak (vocative
    /// of `туча`, "O cloud!"), so the dictionary detector must
    /// switch to en-US `next` when both are dict hits.
    #[test]
    fn bundled_weak_list_marks_tuche() {
        let db = LayoutDb::load_embedded();
        let uk = db
            .get(&LayoutId::from("uk-UA"))
            .and_then(|l| l.dictionary.as_ref())
            .expect("uk dict");
        // Sanity that `туче` IS in the bundled FST — the test would
        // pass vacuously if Hunspell ever stops emitting it.
        assert!(uk.contains("туче"), "`туче` must be in the bundled uk FST");
        assert!(
            uk.is_weak("туче"),
            "`туче` must be on the bundled uk-UA weak list — \
             see data/wordlists/uk_ua-weak.txt"
        );
    }

    #[test]
    fn user_weak_file_extends_weak_list() {
        let tmp = TmpDir::new("weak");
        tmp.write("uk_ua-weak.txt", "# user adds\nтестслабке\n");

        let db = LayoutDb::load_embedded_with_user_overlay(Some(&tmp.0));
        let dict = db
            .get(&LayoutId::from("uk-UA"))
            .and_then(|l| l.dictionary.as_ref())
            .expect("uk dict");
        assert!(
            dict.is_weak("тестслабке"),
            "user-side -weak.txt should extend the weak list"
        );
    }

    #[test]
    fn user_short_stop_file_extends_stop_list() {
        let tmp = TmpDir::new("stop");
        tmp.write("uk_ua-stop.txt", "хм\n");

        let db = LayoutDb::load_embedded_with_user_overlay(Some(&tmp.0));
        let dict = db
            .get(&LayoutId::from("uk-UA"))
            .and_then(|l| l.dictionary.as_ref())
            .expect("uk dict");

        assert!(
            dict.contains_short("хм"),
            "user-side -stop.txt should extend short stop list"
        );
    }

    #[test]
    fn missing_user_files_do_not_break_loading() {
        let tmp = TmpDir::new("empty");
        let db = LayoutDb::load_embedded_with_user_overlay(Some(&tmp.0));
        assert!(db.get(&LayoutId::from("en-US")).is_some());
        assert!(db.get(&LayoutId::from("uk-UA")).is_some());
    }

    fn minimal_layout_toml(id: &str, name: &str, script: &str) -> String {
        format!(
            r#"
id     = "{id}"
name   = "{name}"
script = "{script}"

[keys]
0x10 = {{ plain = "x", shift = "X" }}
0x11 = {{ plain = "y", shift = "Y" }}
"#,
        )
    }

    #[test]
    fn user_layout_dir_adds_extra_layout() {
        let layout_tmp = TmpDir::new("user-layouts-add");
        std::fs::write(
            layout_tmp.0.join("kk_kz.toml"),
            minimal_layout_toml("kk-KZ", "Қазақ", "Cyrillic"),
        )
        .expect("write user layout");

        let db = LayoutDb::load_with_user_layouts(Some(&layout_tmp.0), None);
        assert!(
            db.get(&LayoutId::from("kk-KZ")).is_some(),
            "user-side TOML at <dir>/kk_kz.toml should load as kk-KZ"
        );
        assert!(db.get(&LayoutId::from("en-US")).is_some());
    }

    #[test]
    fn user_layout_overrides_embedded_with_same_id() {
        let layout_tmp = TmpDir::new("user-layouts-override");
        std::fs::write(
            layout_tmp.0.join("en_us.toml"),
            minimal_layout_toml("en-US", "USER-OVERRIDE-EN", "Latin"),
        )
        .expect("write user layout");

        let db = LayoutDb::load_with_user_layouts(Some(&layout_tmp.0), None);
        let en = db.get(&LayoutId::from("en-US")).expect("en-US present");
        assert_eq!(
            en.name, "USER-OVERRIDE-EN",
            "user TOML should win over embedded layout"
        );
    }

    #[test]
    fn malformed_user_layout_is_skipped() {
        let layout_tmp = TmpDir::new("user-layouts-malformed");
        std::fs::write(
            layout_tmp.0.join("bad.toml"),
            "this is not valid TOML at all <<<>>>",
        )
        .expect("write bad layout");

        let db = LayoutDb::load_with_user_layouts(Some(&layout_tmp.0), None);
        assert!(db.get(&LayoutId::from("en-US")).is_some());
        assert!(db.get(&LayoutId::from("bad")).is_none());
    }

    #[test]
    fn user_layout_picks_up_matching_wordlist() {
        let layout_tmp = TmpDir::new("user-layouts-dict-l");
        let overlay_tmp = TmpDir::new("user-layouts-dict-w");
        std::fs::write(
            layout_tmp.0.join("kk_kz.toml"),
            minimal_layout_toml("kk-KZ", "Қазақ", "Cyrillic"),
        )
        .expect("write user layout");
        std::fs::write(overlay_tmp.0.join("kk_kz.txt"), "тілқолданбасы\n")
            .expect("write user wordlist");

        let db = LayoutDb::load_with_user_layouts(Some(&layout_tmp.0), Some(&overlay_tmp.0));
        let dict = db
            .get(&LayoutId::from("kk-KZ"))
            .and_then(|l| l.dictionary.as_ref())
            .expect("kk-KZ dictionary built from overlay");
        assert!(dict.contains("тілқолданбасы"));
    }

    // ─── Plug-in pack loader ───────────────────────────────────────

    /// Minimal but real FST blob for "must compile, must read back"
    /// tests. We don't need a populated dictionary to verify the
    /// loader picks up the file — we just need the bytes to parse as
    /// a valid `FstSet`.
    fn empty_fst_bytes() -> Vec<u8> {
        let builder = fst::SetBuilder::memory();
        builder.into_inner().expect("empty FST builder")
    }

    /// Happy path: drop a complete plug-in tree under
    /// `<data_dir>/plugins/<pack>/` and verify the contained layout
    /// shows up in the loaded `LayoutDb` with its dictionary attached.
    #[test]
    fn plugin_pack_layout_loads() {
        let data_dir = TmpDir::new("plugin-happy");
        let pack_dir = data_dir.0.join("plugins").join("test-pack");
        std::fs::create_dir_all(pack_dir.join("layout-mappings")).unwrap();
        std::fs::create_dir_all(pack_dir.join("wordlists")).unwrap();

        std::fs::write(
            pack_dir.join("manifest.toml"),
            r#"
id = "test-pack"
name = "Test pack"
version = "0.0.1"
supported_layouts = ["kk-KZ"]
"#,
        )
        .unwrap();
        std::fs::write(
            pack_dir.join("layout-mappings").join("kk_kz.toml"),
            minimal_layout_toml("kk-KZ", "Қазақ", "Cyrillic"),
        )
        .unwrap();
        std::fs::write(
            pack_dir.join("wordlists").join("kk_kz.fst"),
            empty_fst_bytes(),
        )
        .unwrap();

        let db = LayoutDb::load(LoadOptions {
            data_dir: Some(&data_dir.0),
            ..Default::default()
        })
        .expect("load plugin pack");

        let layout = db
            .get(&LayoutId::from("kk-KZ"))
            .expect("plug-in's kk-KZ layout should be loaded");
        // Dictionary attached because we shipped an FST in the pack.
        assert!(
            layout.dictionary.is_some(),
            "plug-in with shipped FST should have a dictionary"
        );
    }

    /// A plug-in directory without a `manifest.toml` is skipped, but
    /// the rest of the load proceeds. Without this guard, a stray
    /// folder under `plugins/` (e.g. `.git/`, `__MACOSX/` from an
    /// extracted zip) could derail every other pack.
    #[test]
    fn plugin_missing_manifest_skipped_gracefully() {
        let data_dir = TmpDir::new("plugin-no-manifest");
        let pack_dir = data_dir.0.join("plugins").join("broken-pack");
        std::fs::create_dir_all(pack_dir.join("layout-mappings")).unwrap();
        // No manifest.toml. There's even a TOML in layout-mappings
        // that *would* parse — but since the pack lacks a manifest
        // we should refuse to load anything from it.
        std::fs::write(
            pack_dir.join("layout-mappings").join("kk_kz.toml"),
            minimal_layout_toml("kk-KZ", "Қазақ", "Cyrillic"),
        )
        .unwrap();

        let db = LayoutDb::load(LoadOptions {
            data_dir: Some(&data_dir.0),
            ..Default::default()
        })
        .expect("load with broken pack");
        assert!(
            db.get(&LayoutId::from("kk-KZ")).is_none(),
            "layout from a manifest-less pack must not be loaded"
        );
    }

    /// Plug-in's TOML with an unparseable manifest is skipped. The
    /// pack is logged + ignored; other packs in the same plug-ins
    /// directory keep loading.
    #[test]
    fn plugin_invalid_manifest_skipped() {
        let data_dir = TmpDir::new("plugin-bad-manifest");
        let bad_pack = data_dir.0.join("plugins").join("bad-pack");
        std::fs::create_dir_all(bad_pack.join("layout-mappings")).unwrap();
        std::fs::write(bad_pack.join("manifest.toml"), "not = valid = toml === ").unwrap();
        std::fs::write(
            bad_pack.join("layout-mappings").join("kk_kz.toml"),
            minimal_layout_toml("kk-KZ", "Қазақ", "Cyrillic"),
        )
        .unwrap();

        // A second pack alongside, this one well-formed — must still
        // load to prove the bad pack didn't poison the whole load.
        let good_pack = data_dir.0.join("plugins").join("good-pack");
        std::fs::create_dir_all(good_pack.join("layout-mappings")).unwrap();
        std::fs::create_dir_all(good_pack.join("wordlists")).unwrap();
        std::fs::write(
            good_pack.join("manifest.toml"),
            r#"id="good-pack"
name="Good"
version="0.1.0""#,
        )
        .unwrap();
        std::fs::write(
            good_pack.join("layout-mappings").join("kk_kz.toml"),
            minimal_layout_toml("kk-KZ", "Қазақ from good pack", "Cyrillic"),
        )
        .unwrap();
        std::fs::write(
            good_pack.join("wordlists").join("kk_kz.fst"),
            empty_fst_bytes(),
        )
        .unwrap();

        let db = LayoutDb::load(LoadOptions {
            data_dir: Some(&data_dir.0),
            ..Default::default()
        })
        .expect("load with mixed packs");
        let layout = db
            .get(&LayoutId::from("kk-KZ"))
            .expect("good pack should still load even though bad pack lives next to it");
        assert_eq!(
            layout.name, "Қазақ from good pack",
            "the good pack's layout should win — not the bad pack's TOML"
        );
    }

    /// User overlay TOML overrides a plug-in TOML with the same id.
    /// This is the documented precedence chain
    /// `bundled ← plug-ins ← user-overlay`. A user dropping a
    /// matching TOML under `<config-dir>/kb-switcher/layouts/` should
    /// always win.
    #[test]
    fn user_overlay_overrides_plugin() {
        let data_dir = TmpDir::new("plugin-vs-user-data");
        let user_dir = TmpDir::new("plugin-vs-user-layouts");

        let pack = data_dir.0.join("plugins").join("p");
        std::fs::create_dir_all(pack.join("layout-mappings")).unwrap();
        std::fs::create_dir_all(pack.join("wordlists")).unwrap();
        std::fs::write(
            pack.join("manifest.toml"),
            r#"id="p"
name="Pack"
version="0.0.1""#,
        )
        .unwrap();
        std::fs::write(
            pack.join("layout-mappings").join("kk_kz.toml"),
            minimal_layout_toml("kk-KZ", "FROM-PACK", "Cyrillic"),
        )
        .unwrap();
        std::fs::write(pack.join("wordlists").join("kk_kz.fst"), empty_fst_bytes()).unwrap();

        // The user's own copy of `kk-KZ`.
        std::fs::write(
            user_dir.0.join("kk_kz.toml"),
            minimal_layout_toml("kk-KZ", "FROM-USER", "Cyrillic"),
        )
        .unwrap();

        let db = LayoutDb::load(LoadOptions {
            data_dir: Some(&data_dir.0),
            user_layout_dir: Some(&user_dir.0),
            ..Default::default()
        })
        .expect("load with plugin + user overlap");
        let layout = db.get(&LayoutId::from("kk-KZ")).expect("kk-KZ");
        assert_eq!(
            layout.name, "FROM-USER",
            "user overlay must win over plug-in for the same id"
        );
    }

    #[test]
    fn user_layout_without_wordlist_still_loads() {
        let layout_tmp = TmpDir::new("user-layouts-nodict-l");
        let overlay_tmp = TmpDir::new("user-layouts-nodict-w");
        std::fs::write(
            layout_tmp.0.join("kk_kz.toml"),
            minimal_layout_toml("kk-KZ", "Қазақ", "Cyrillic"),
        )
        .expect("write user layout");

        let db = LayoutDb::load_with_user_layouts(Some(&layout_tmp.0), Some(&overlay_tmp.0));
        let layout = db.get(&LayoutId::from("kk-KZ")).expect("kk-KZ loaded");
        assert!(
            layout.dictionary.is_none(),
            "no overlay file → no dictionary attached"
        );
    }

    #[test]
    fn overlay_is_freshly_read_on_each_build() {
        let tmp = TmpDir::new("reload");
        let first_token = "zxqzxqfirst";
        let second_token = "qwrqwrsecond";

        tmp.write("uk_ua.txt", &format!("{first_token}\n"));
        let first = LayoutDb::load_embedded_with_user_overlay(Some(&tmp.0));
        let first_dict = first
            .get(&LayoutId::from("uk-UA"))
            .and_then(|l| l.dictionary.as_ref())
            .expect("uk dict #1");
        assert!(first_dict.contains(first_token));
        assert!(!first_dict.contains(second_token));

        tmp.write("uk_ua.txt", &format!("{second_token}\n"));
        let second = LayoutDb::load_embedded_with_user_overlay(Some(&tmp.0));
        let second_dict = second
            .get(&LayoutId::from("uk-UA"))
            .and_then(|l| l.dictionary.as_ref())
            .expect("uk dict #2");
        assert!(second_dict.contains(second_token));
        assert!(
            !second_dict.contains(first_token),
            "old overlay must not leak into the fresh load"
        );
    }
}
