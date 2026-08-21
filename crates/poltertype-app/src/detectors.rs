//! Detector construction and dictionary (re)loading.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use poltertype_core::layouts::LayoutDb;
use poltertype_core::settings::SettingsStore;
use poltertype_core::wordlist_profiles::{WordlistSettings, resolve_active_profile};
use poltertype_detect::{DictionaryDetector, WordPlausibilityDetector};
use poltertype_types::LayoutId;
use tracing::{info, warn};

use crate::types::*;

/// Build one dictionary set per configured wordlist profile, ready to
/// swap in when focus enters the matching app. No profiles → empty cache
/// and no watcher. Each profile reuses the bundled FSTs through the `Arc`
/// inside `LayoutDictionary`; only the user overlays are re-derived.
pub(crate) fn build_profile_dictionary_cache(
    layouts: &Arc<LayoutDb>,
    data_dir: &std::path::Path,
    wordlists: &WordlistSettings,
) -> HashMap<String, HashMap<LayoutId, poltertype_detect::LayoutDictionary>> {
    let mut out: HashMap<String, HashMap<LayoutId, poltertype_detect::LayoutDictionary>> =
        HashMap::new();
    for profile in &wordlists.profiles {
        let Some(dir) = poltertype_core::layouts::user_profile_wordlist_dir(&profile.id) else {
            warn!(
                profile = %profile.id,
                "no config dir resolved; profile cache entry skipped"
            );
            continue;
        };
        let dicts = layouts.build_profile_dictionaries(data_dir, &dir);
        info!(
            profile = %profile.id,
            ?dir,
            dicts = dicts.len(),
            "profile dictionaries cached"
        );
        out.insert(profile.id.clone(), dicts);
    }
    out
}

/// Poll `FocusTracker::focused_exe()` every ~250 ms and swap the
/// dictionary set when the resolved profile changes, or when
/// `force_reapply` is set.
///
/// `force_reapply` covers the case "swap on change" misses: the cache was
/// rebuilt while the user stayed on the same app, which is what happens
/// when they save wordlist edits from the Settings UI.
///
/// Transient tracker errors are swallowed — a flaky Wayland tracker is
/// not worth log spam and the next poll catches up.
pub(crate) fn spawn_profile_watcher(
    focus_tracker: Arc<dyn poltertype_input::FocusTracker>,
    settings: Arc<SettingsStore>,
    profile_cache: ProfileDictCache,
    force_reapply: Arc<AtomicBool>,
    dict_handle: poltertype_detect::DictionaryDetector,
) -> Result<()> {
    std::thread::Builder::new()
        .name("kb-profile-watcher".into())
        .spawn(move || {
            // Empty string = "no profile / global overlay active".
            let mut active: String = String::new();
            loop {
                let exe = focus_tracker.focused_exe();
                let basename = exe.as_deref().and_then(|e| {
                    std::path::Path::new(e)
                        .file_name()
                        .and_then(|f| f.to_str())
                });
                let snap = settings.snapshot();
                let resolved = resolve_active_profile(&snap.wordlists, basename)
                    .map(str::to_owned)
                    .unwrap_or_default();

                let forced = force_reapply.swap(false, Ordering::AcqRel);
                if resolved != active || forced {
                    let dicts_opt = profile_cache.read().get(&resolved).cloned();
                    if let Some(dicts) = dicts_opt {
                        info!(
                            previous = %active,
                            new_profile = if resolved.is_empty() { "<global>" } else { resolved.as_str() },
                            dicts = dicts.len(),
                            forced,
                            "wordlist profile (re-)applied"
                        );
                        dict_handle.replace_dicts(dicts);
                    } else {
                        warn!(
                            profile = %resolved,
                            "resolved profile has no cache entry; keeping current dicts"
                        );
                    }
                    active = resolved;
                }

                std::thread::sleep(Duration::from_millis(250));
            }
        })
        .context("spawn profile watcher thread")?;
    Ok(())
}

/// Build the full per-profile cache, including the global baseline under
/// the empty-string key.
///
/// That key is what the watcher swaps back to when focus leaves a
/// profiled app; without it, moving from VS Code to Chrome would keep
/// the code overlay loaded for ever.
pub(crate) fn build_full_profile_cache(
    layouts: &Arc<LayoutDb>,
    data_dir: &Path,
    wordlists: &WordlistSettings,
    user_wordlist_dir: Option<&Path>,
) -> HashMap<String, HashMap<LayoutId, poltertype_detect::LayoutDictionary>> {
    let mut cache = build_profile_dictionary_cache(layouts, data_dir, wordlists);
    if !cache.is_empty() {
        let global = layouts
            .build_profile_dictionaries(data_dir, user_wordlist_dir.unwrap_or(Path::new("")));
        cache.insert(String::new(), global);
    }
    cache
}

pub(crate) fn build_plausibility_detector(layouts: &Arc<LayoutDb>) -> WordPlausibilityDetector {
    let profiles = layouts
        .iter()
        .map(|(id, m)| (id.clone(), m.detector_profile()))
        .collect();
    WordPlausibilityDetector::new(profiles)
}

pub(crate) fn build_dictionary_detector(layouts: &Arc<LayoutDb>) -> DictionaryDetector {
    DictionaryDetector::new(collect_dicts(layouts))
}

/// Build the spelling-suggestion provider. Shares the dictionary
/// detector's hot-swappable set, so profile swaps and "Reload Settings"
/// reach suggestions instantly, and derives one keyboard geometry per
/// layout — the ranking metric's "was this a finger slip?" signal.
pub(crate) fn build_suggester(
    layouts: &LayoutDb,
    dicts: DictionaryDetector,
) -> Arc<poltertype_detect::Suggester> {
    let geometry = layouts
        .iter()
        .map(|(id, m)| {
            let pairs = m.keys.iter().flat_map(|(&sc, &(plain, shift))| {
                std::iter::once((sc, plain)).chain(shift.map(|s| (sc, s)))
            });
            (
                id.clone(),
                poltertype_detect::KeyboardGeometry::from_scancode_chars(pairs),
            )
        })
        .collect();
    Arc::new(poltertype_detect::Suggester::new(dicts, geometry))
}

pub(crate) fn collect_dicts(
    layouts: &LayoutDb,
) -> std::collections::HashMap<poltertype_types::LayoutId, poltertype_detect::LayoutDictionary> {
    layouts
        .iter()
        .filter_map(|(id, m)| m.dictionary.as_ref().map(|d| (id.clone(), d.clone())))
        .collect()
}

/// Append `word` to the user's global overlay for `layout` and insert it
/// into the running set in place. Deliberately not a full
/// `reload_user_dictionaries`, which re-reads and re-leaks every FST blob.
///
/// Known edge: while a per-app profile is active, the next profile swap
/// replaces the in-memory set with its startup-built cache and hides
/// the word until a restart. The file keeps it durable either way.
pub(crate) fn add_word_to_user_overlay(
    layout: &poltertype_types::LayoutId,
    word: &str,
    handle: &DictionaryDetector,
) -> anyhow::Result<()> {
    use std::io::Write as _;
    // One shape on disk and in memory. `parse_wordlist` runs every line
    // back through `letters_only_lower`, so writing the raw token left
    // the file saying `just-code.net` where the dictionary held
    // `justcodenet`.
    let word = poltertype_detect::letters_only_lower(word);
    let dir = crate::user_dirs::ensure_user_wordlist_dir()?;
    let stem = layout.as_str().to_lowercase().replace('-', "_");
    let path = dir.join(format!("{stem}.txt"));
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    writeln!(f, "{word}")?;
    let live = handle.add_overlay_word(layout, &word);
    // The word itself stays out of the log, as everywhere else.
    tracing::info!(%stem, live, "added a word to the user wordlist overlay");
    Ok(())
}

/// Re-read the user's wordlist overlays from disk and atomically swap the
/// engine's dictionary set; returns how many loaded.
///
/// Only **global overlays for already-loaded layouts** are picked up
/// live — the load-bearing case, adding vocabulary like `kubectl`.
/// Brand-new user layouts, per-profile overlays, `[[wordlists.profiles]]`
/// schema changes and hotkey rebinds all need a restart: the engine holds
/// a snapshot `Arc<LayoutDb>`, the profile cache is built once at
/// startup, and the two built-in hotkeys are registered with the OS once.
///
/// `[[commands]]` text triggers are the exception: the engine reads them
/// from `settings.snapshot()` on every word boundary.
pub(crate) fn reload_user_dictionaries(handle: &DictionaryDetector) -> usize {
    let wordlist_dir = poltertype_core::layouts::user_wordlist_dir();
    let layout_dir = poltertype_core::layouts::user_layout_dir();
    let new_layouts =
        LayoutDb::load_with_user_layouts(layout_dir.as_deref(), wordlist_dir.as_deref());
    let new_dicts = collect_dicts(&new_layouts);
    let n = new_dicts.len();
    handle.replace_dicts(new_dicts);
    info!(
        loaded = n,
        wordlist_overlay = ?wordlist_dir,
        layout_overlay = ?layout_dir,
        "user wordlist overlays reloaded"
    );
    n
}

// ─── AI plug-ins ──────────────────────────────────────────────────────

/// Detectors declared under `[[ai.plugins]]`, or nothing at all.
///
/// Three gates stand between a config file and a detector voting, and
/// all three must be open: the binary was built with `--features ai`
/// (otherwise this is the `#[cfg]`-ed-out version below),
/// `[ai].enabled = true`, and the individual entry builds — see
/// `poltertype_ai::build_detectors`, which skips a bad entry rather
/// than losing the others. Remote plug-ins carry a fourth,
/// `[ai].allow_remote`, checked per judgement so flipping it needs no
/// config edit.
///
/// The result is *appended* to the built-in detectors, never
/// substituted: the offline pipeline keeps working exactly as before if
/// the AI voice says nothing.
#[cfg(feature = "ai")]
pub(crate) fn build_ai_detectors(
    ai: &poltertype_core::settings::AiSettings,
) -> Vec<Box<dyn poltertype_detect::Detector>> {
    if !ai.enabled {
        // Silent on purpose: this is the default, and a log line about a
        // feature nobody asked for is noise.
        return Vec::new();
    }
    if ai.plugins.is_empty() {
        info!("[ai].enabled = true but no [[ai.plugins]] are configured; nothing to load");
        return Vec::new();
    }
    let built = poltertype_ai::build_detectors(&ai.plugins, ai.allow_remote);
    info!(
        configured = ai.plugins.len(),
        loaded = built.len(),
        allow_remote = ai.allow_remote,
        "AI detectors ready"
    );
    built
}

/// Without the `ai` feature the crate is not even linked, so there is
/// nothing to build. The settings still parse — a config file must not
/// stop being readable because of how the binary was compiled.
#[cfg(not(feature = "ai"))]
pub(crate) fn build_ai_detectors(
    ai: &poltertype_core::settings::AiSettings,
) -> Vec<Box<dyn poltertype_detect::Detector>> {
    if ai.enabled {
        warn!(
            "[ai].enabled = true, but this build has no AI subsystem compiled in \
             (build with `--features ai`). Continuing without it."
        );
    }
    Vec::new()
}

#[cfg(test)]
mod tests;
