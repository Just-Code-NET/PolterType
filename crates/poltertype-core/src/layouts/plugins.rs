//! Plug-in language packs: enumerate `<data_dir>/plugins/*/`
//! and merge each pack's layouts into the load.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use poltertype_types::LayoutId;
use tracing::{info, warn};

use super::files::build_dictionary;
use super::types::{LayoutMapping, PluginManifest};

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
pub fn load_plugin_packs(
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

pub fn load_one_pack(
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
