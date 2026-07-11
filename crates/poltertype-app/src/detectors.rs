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

/// Build one dictionary set per configured wordlist profile, ready
/// to swap into [`poltertype_detect::DictionaryDetector`] when focus enters
/// the matching app(s). Empty `wordlists.profiles` → empty cache;
/// the focus watcher is then never spawned, so this is zero-cost
/// for the common no-profile case.
///
/// Each profile reuses the bundled FSTs through the `Arc` inside
/// `LayoutDictionary` — only the user-overlay HashSets are
/// re-derived. So building 5 profiles takes 5 × (number-of-layouts)
/// disk-cheap text-file reads, not 5 × FST decode.
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

/// Poll `FocusTracker::focused_exe()` every ~250 ms; swap the
/// dictionary set when the resolved profile changes OR when the
/// `force_reapply` flag has been set. Same cadence as
/// `spawn_layout_poller` so the two stay in lock-step at the human
/// perception level (the user perceives "I focused VS Code, my code
/// dictionary kicked in").
///
/// The `force_reapply` flag exists because the watcher's normal
/// "swap on profile change" rule misses the case where the cache
/// itself was rebuilt while the user stayed on the same app. That
/// happens when the user saves wordlist edits via the Settings UI
/// while focused on a profiled app — the close-handler rebuilds the
/// cache, but the resolved profile didn't change, so the watcher
/// would otherwise sit on stale dicts until the user alt-tabbed.
/// Setting the flag forces one re-apply on the next tick.
///
/// The poller swallows transient FocusTracker errors silently —
/// a flaky Wayland tracker isn't worth log spam, and the next
/// successful poll catches up. We log once per profile *transition*
/// so the user can verify in the log file that swaps are happening.
///
/// Profile cache + dict_reload_handle are cloned cheaply (Arc
/// internals); the thread owns its copies and runs forever.
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
                    // The cache always holds the empty-string ("")
                    // key as the global baseline, so a profile
                    // transition — including back to global — is
                    // always a single map lookup + swap. `forced`
                    // means the cache itself was rebuilt while the
                    // resolved profile didn't change (e.g. user
                    // saved wordlist edits via the GUI); we still
                    // re-apply the same key so the engine sees the
                    // fresh dicts.
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

/// Build the full per-profile dictionary cache including the
/// global-baseline entry under the empty-string key. Called both
/// at startup (initial cache) and from the Settings UI close
/// handler (after user saves wordlist edits, to pick them up
/// without a tray restart).
///
/// The empty-string key is critical: without it, the watcher
/// would have nowhere to swap back to when focus leaves a
/// profiled app, so e.g. moving from VS Code (profile=`code`) to
/// Chrome would keep the code overlay loaded forever — opposite
/// of the user's intent. Adding it is cheap (just one more pass
/// through the layouts) so we always include it once any profile
/// is configured.
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

pub(crate) fn collect_dicts(
    layouts: &LayoutDb,
) -> std::collections::HashMap<poltertype_types::LayoutId, poltertype_detect::LayoutDictionary> {
    layouts
        .iter()
        .filter_map(|(id, m)| m.dictionary.as_ref().map(|d| (id.clone(), d.clone())))
        .collect()
}

/// Re-read `<config-dir>/poltertype/wordlists/<stem>.txt` from disk
/// and atomically swap the engine's dictionary set. Returns the
/// number of dictionaries successfully loaded. Always rebuilds — even
/// when the user added zero new entries — so the user gets a clear
/// signal in the log that the reload took effect.
///
/// Scope of the reload:
///
/// * **Global wordlist overlays** for already-loaded layouts →
///   picked up immediately (this is the load-bearing case — adding
///   tech vocab like `kubectl`, `terraform`, …).
/// * **Brand-new user layouts** (a freshly-dropped TOML in
///   `<config-dir>/poltertype/layouts/`) → require an app restart.
///   The engine holds a snapshot `Arc<LayoutDb>`, so the new layout
///   wouldn't be in its scancode-translation tables anyway. We log
///   loud-and-clear if we see one, so the user knows.
/// * **Per-profile wordlist overlays**
///   (`<config-dir>/poltertype/wordlists/profiles/<id>/<stem>.txt`)
///   → require an app restart. The profile dictionary cache built
///   at startup isn't rebuilt by Reload Settings; the focus-watcher
///   re-applies the cached set on the next focus transition. The
///   Wordlists pane already tells users to restart for profile
///   edits; this matches that contract.
/// * **`[[wordlists.profiles]]` schema changes** → require an app
///   restart. The profile cache is built once at startup; adding a
///   new profile entry without restarting means the cache has no
///   dictionary set for it and the focus-watcher can't activate it.
/// * **`[[commands]]` schema changes** → live for text triggers,
///   restart for hotkey rebinds. The engine reads the commands
///   list from `settings.snapshot()` on every word boundary, so a
///   text-trigger command added/removed via the Settings UI takes
///   effect on the next typed word (the parent SettingsStore
///   reloads config.toml when the GUI subprocess exits). The two
///   built-in hotkeys (`[hotkeys].pause_toggle` /
///   `manual_switch_last`) are registered with the OS once at
///   startup, so rebinding them still needs a tray restart.
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
