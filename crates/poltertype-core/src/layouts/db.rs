//! `LayoutDb` — the handle to every loaded layout.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use poltertype_types::LayoutId;
use tracing::{info, warn};

use super::consts::BUNDLED_LAYOUT_STEMS;
use super::enums::LayoutLoadError;
use super::files::*;
use super::helpers::{compute_letter_scancodes, peek_layout_id};
use super::os_keymap::apply_os_keymaps;
use super::plugins::load_plugin_packs;
use super::types::{LayoutMapping, LoadOptions};

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
    /// Top-level loader: resolves the data directory if not supplied,
    /// scans bundled stems and optionally user-side TOMLs.
    ///
    /// Bundled layouts outside `active_filter` are skipped and their
    /// FSTs never enter memory. User layouts always load — dropping a
    /// TOML in `<config-dir>/layouts/` is an explicit request.
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

        // Loaded before user-side TOMLs, so precedence runs
        // bundled ← plug-ins ← user-overlay and a user can still
        // override a plug-in with a TOML of the same id.
        // See docs/DATA_LAYOUT.md.
        load_plugin_packs(data_dir, opts.user_wordlist_dir, &mut by_id);

        // What the OS says these keyboards really do: applied over
        // everything we shipped and under the user's own TOMLs. A
        // bundled mapping describes *a* keyboard for the language, this
        // describes *the* one on this machine — see `os_keymap`.
        if let Some(keymaps) = opts.os_keymaps {
            apply_os_keymaps(keymaps, &mut by_id);
        }

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

    /// Convenience: every bundled layout from the auto-resolved data
    /// dir, no user overlay, no active filter. For tests and tooling;
    /// production uses [`Self::load`] with the OS active-layouts list.
    ///
    /// Panics on data-dir resolution failure — there is nothing useful
    /// to do without it. Callers that want to recover use
    /// [`Self::load`] and handle the `Result`.
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

    /// Build a profile-specific dictionary set — the shape
    /// `DictionaryDetector::replace_dicts` accepts — from the overlay
    /// files in `profile_overlay_dir` rather than the global one.
    ///
    /// Each entry reuses the layout's bundled FST through the `Arc` in
    /// `LayoutDictionary`, so a caller can build every profile's set at
    /// startup, cache it, and swap atomically on focus change without
    /// rebuilding any FST.
    ///
    /// Layouts with no FST in the data dir get an overlay-only
    /// dictionary if the profile mentions them, and are skipped if not.
    ///
    /// Takes the data dir as a parameter rather than resolving it:
    /// resolving internally would lose the explicit-config-dir test
    /// path, and the caller already has it from `LoadOptions`.
    pub fn build_profile_dictionaries(
        &self,
        data_dir: &Path,
        profile_overlay_dir: &Path,
    ) -> HashMap<LayoutId, poltertype_detect::LayoutDictionary> {
        let mut out = HashMap::new();
        for (id, mapping) in &self.by_id {
            // Bundled stems are the BCP-47 id lowercased with `-` → `_`,
            // the same convention the FST file names follow. User
            // layouts' real stem is their TOML filename, which is not
            // tracked here, so they fall back to the same shape — which
            // most of them follow, so profile overlays still apply.
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
