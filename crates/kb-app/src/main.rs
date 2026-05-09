//! kb-switcher application entry point.
//!
//! Wires the tray + global keyboard listener + layout switcher +
//! `SwitcherEngine` together, registers the two built-in global
//! hotkeys (pause / switch-last), and spawns the focus-driven
//! wordlist-profile watcher when the user has profiles configured.
//!
//! The Settings GUI is a **separate process** spawned via
//! `kb-switcher --settings` — see `settings_ui.rs` for the
//! rationale (macOS main-thread contention, crash isolation).
//! User-defined "smart commands" (`[[commands]]` in `config.toml`)
//! are NOT wired here as global hotkeys; they're text triggers
//! consulted by the engine on every word boundary. See
//! `kb_core::commands` for the design.

#![forbid(unsafe_code)]

mod icon_render;
mod settings_ui;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use parking_lot::RwLock;

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use global_hotkey::hotkey::{Code, HotKey, Modifiers as HkMods};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use kb_core::audio::AudioPlayer;
use kb_core::engine::{EngineCommand, SwitcherEngine, SwitcherEvent};
use kb_core::layouts::LayoutDb;
use kb_core::settings::SettingsStore;
use kb_core::wordlist_profiles::{WordlistSettings, resolve_active_profile};
use kb_detect::{Detector, DictionaryDetector, WordPlausibilityDetector};
use kb_input::{KeyEvent, create_emitter, create_focus_tracker, create_listener};
use kb_layout::create_switcher;
use kb_types::LayoutId;
use single_instance::SingleInstance;
use tao::event::Event;
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tracing::{debug, error, info, warn};
use tray_icon::TrayIcon;
use tray_icon::TrayIconBuilder;
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};

const APP_ID: &str = "dev.opensource.kb-switcher";
const APP_NAME: &str = "kb-switcher";

#[derive(Debug, Clone)]
enum UserEvent {
    Menu(MenuId),
    Hotkey(u32),
    Engine(SwitcherEvent),
}

fn main() -> Result<()> {
    // CLI dispatch: `kb-switcher --settings` opens the Settings GUI
    // and exits when the window closes. Anything else falls through
    // to the tray. We do this BEFORE `init_tracing` / single-instance
    // because:
    //
    // * The settings UI is a short-lived child process spawned by the
    //   tray. Hitting the single-instance lock would kill it on
    //   startup; logging would steal the tray's log file rotation.
    // * `--help` / `--version` need to be cheap and side-effect-free.
    let mut args = std::env::args().skip(1);
    if let Some(arg) = args.next() {
        match arg.as_str() {
            "--settings" | "-s" | "settings" => return settings_ui::run(),
            "--version" | "-V" => {
                println!("{APP_NAME} {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--help" | "-h" => {
                print_help();
                return Ok(());
            }
            other => {
                eprintln!("kb-switcher: unknown argument `{other}`");
                print_help();
                return Err(anyhow::anyhow!("unknown CLI argument"));
            }
        }
    }

    let _log_guard = init_tracing();
    info!(version = env!("CARGO_PKG_VERSION"), "{APP_NAME} starting");

    let instance = SingleInstance::new(APP_ID).context("create single-instance lock")?;
    if !instance.is_single() {
        warn!("another instance is already running, exiting");
        return Ok(());
    }

    // ─── Settings ──────────────────────────────────────────────────
    let settings = match SettingsStore::load_or_default() {
        Ok(s) => Arc::new(s),
        Err(e) => {
            error!(?e, "could not load settings; aborting startup");
            return Err(anyhow::anyhow!(e));
        }
    };
    info!(path = ?settings.path(), "settings loaded");

    // ─── Layout switcher (built first so we can query active OS
    //                     layouts before loading the DB) ────────────
    let layout_switcher: Arc<dyn kb_layout::LayoutSwitcher> = match create_switcher() {
        Ok(s) => {
            info!(backend = s.backend_name(), "layout switcher ready");
            Arc::from(s)
        }
        Err(e) => {
            error!(?e, "no layout switcher backend; aborting");
            return Err(anyhow::anyhow!(e));
        }
    };

    // ─── Layouts ───────────────────────────────────────────────────
    // We now ship layout mappings + FST wordlists as plain files in
    // a `data/` directory next to the executable (Windows MSI),
    // inside `Contents/Resources/data/` (macOS .app), or
    // `usr/share/kb-switcher/data/` (Linux AppImage). The runtime
    // resolver in `kb_core::data_dir` figures out which path is
    // live; in dev mode it falls back to `target/dist/data/` where
    // `kb-core/build.rs` writes prepared assets.
    //
    // We then ask the OS which layouts the user has actually
    // enabled (`list_active`) and only load **those** wordlists into
    // memory. A user with `en-US / uk-UA / ru-RU` saves the FST RAM
    // for the four other bundled languages they'd never query — and
    // the detector can no longer pick an unreachable layout (the
    // root cause of the original `http ` bug).
    let data_dir = kb_core::resolve_data_dir().context("resolve data directory")?;
    info!(?data_dir, "data directory resolved");

    let active_os_layouts = match layout_switcher.list_active() {
        Ok(list) => {
            info!(active = ?list, count = list.len(), "OS active layouts");
            Some(list)
        }
        Err(e) => {
            // Fail-open: we can't decide what's reachable, so load
            // every bundled layout (the previous baked-in behaviour).
            // The detector + apply_correction pre-flight guard will
            // still catch any unreachable target at runtime.
            warn!(
                ?e,
                "could not query active OS layouts; loading every bundled layout"
            );
            None
        }
    };

    let user_wordlist_dir = kb_core::layouts::user_wordlist_dir();
    let user_layout_dir = kb_core::layouts::user_layout_dir();
    let layouts = Arc::new(
        LayoutDb::load(kb_core::layouts::LoadOptions {
            data_dir: Some(&data_dir),
            active_filter: active_os_layouts.as_deref(),
            user_layout_dir: user_layout_dir.as_deref(),
            user_wordlist_dir: user_wordlist_dir.as_deref(),
        })
        .context("load layout DB")?,
    );
    info!(
        loaded = layouts.len(),
        ids = ?layouts.ids().collect::<Vec<_>>(),
        wordlist_overlay = ?user_wordlist_dir,
        layout_overlay = ?user_layout_dir,
        "layout DB ready"
    );
    let key_emitter = match create_emitter() {
        Ok(e) => {
            info!(backend = e.backend_name(), "key emitter ready");
            Arc::from(e)
        }
        Err(e) => {
            warn!(?e, "no key emitter backend; corrections will be no-op");
            Arc::from(noop_emitter()) as Arc<dyn kb_input::KeyEmitter>
        }
    };
    let audio = Arc::new(AudioPlayer::new());
    audio.refresh_from(&settings);

    let focus_tracker = create_focus_tracker();
    info!(
        backend = focus_tracker.backend_name(),
        "focus tracker ready"
    );

    // Detector pipeline: dictionary first (highest signal — catches
    // single-letter prepositions and tie-breaks "both look plausible"
    // tokens), word-plausibility second as a fallback for tokens that
    // aren't in either dictionary. Both are pure functions; engine
    // runs them in order and stops at the first non-NoOpinion verdict.
    let dictionary = build_dictionary_detector(&layouts);
    // Cloned handle — shares the inner Arc<RwLock> with the
    // detector that lives inside the engine. Used by the
    // "Reload Settings" path to swap in fresh dictionaries
    // (re-reading user-overlay files) without restarting, AND by
    // the focus-driven wordlist profile watcher below to swap
    // per-app overlays as the user moves between editors / chat /
    // browser / IDE.
    let dict_reload_handle = dictionary.handle();
    let detectors: Vec<Box<dyn Detector>> = vec![
        Box::new(dictionary),
        Box::new(build_plausibility_detector(&layouts)),
    ];

    // ── Wordlist profile cache + focus watcher ───────────────────────
    //
    // Build one dictionary set per configured `[[wordlists.profiles]]`
    // entry up front. The FSTs are already Arc-shared inside
    // LayoutDictionary, so this is "rebuild the user-overlay HashSets
    // once per profile" — milliseconds, even for 5+ profiles.
    //
    // The focus watcher thread (spawned right after the engine is
    // running) polls `focus_tracker.focused_exe()` every ~250 ms,
    // resolves the active profile via `wordlist_profiles::resolve`,
    // and atomically swaps the dictionary set when it changes. The
    // swap is a single `RwLock::write()` — same primitive the manual
    // "Reload Settings" path uses.
    // Profile cache is shared (Arc<RwLock>) so the close-handler in
    // `spawn_settings_ui` can rebuild it from disk when the user
    // saves wordlist edits via the GUI; without that, per-profile
    // wordlist edits would only apply after a tray restart.
    let profile_dict_cache: ProfileDictCache = Arc::new(RwLock::new(build_full_profile_cache(
        &layouts,
        &data_dir,
        &settings.snapshot().wordlists,
        user_wordlist_dir.as_deref(),
    )));
    info!(
        profiles = profile_dict_cache.read().len(),
        "wordlist profile cache built (including global baseline)"
    );

    // Force-reapply flag: set by the close-handler after rebuilding
    // the cache so the watcher re-applies on its next tick (~250 ms)
    // even though the resolved profile didn't change. Without this
    // the watcher only swaps on profile transitions, which means a
    // user editing words while focused on a profiled app would see
    // no effect until they alt-tabbed away and back.
    let profile_force_reapply: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));

    // ─── Engine ────────────────────────────────────────────────────
    let (key_tx, key_rx) = bounded::<KeyEvent>(1024);
    let (engine_event_tx, engine_event_rx) = unbounded::<SwitcherEvent>();
    let (engine_cmd_tx, engine_cmd_rx) = unbounded::<EngineCommand>();

    // Clone the event sender before handing it to the engine — the
    // layout poller below also publishes LayoutChanged events through
    // the same channel.
    let engine_event_tx_for_poller = engine_event_tx.clone();

    let engine = SwitcherEngine::new(
        Arc::clone(&settings),
        Arc::clone(&layouts),
        detectors,
        Arc::clone(&layout_switcher),
        Arc::clone(&key_emitter),
        Arc::clone(&focus_tracker),
        Arc::clone(&audio),
        engine_event_tx,
    );
    std::thread::Builder::new()
        .name("kb-switcher-engine".into())
        .spawn(move || engine.run(key_rx, engine_cmd_rx))
        .context("spawn engine thread")?;

    // ─── Input listener ────────────────────────────────────────────
    let mut input_listener = create_listener().ok();
    if let Some(listener) = input_listener.as_mut() {
        if let Err(e) = listener.start(key_tx) {
            warn!(?e, "input listener failed to start");
        } else {
            info!(backend = listener.backend_name(), "input listener started");
        }
    } else {
        warn!("no input listener backend; engine will receive no events");
    }

    // ─── Tao event loop + tray + global hotkeys ────────────────────
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();

    let menu = Menu::new();
    let item_settings_ui = MenuItem::new("Settings…", true, None);
    let item_settings_file = MenuItem::new("Edit config.toml…", true, None);
    let item_logs = MenuItem::new("Open Logs Folder…", true, None);
    let item_wordlists = MenuItem::new("Open User Wordlists Folder…", true, None);
    let item_layouts = MenuItem::new("Open User Layouts Folder…", true, None);
    let item_reload = MenuItem::new("Reload Settings", true, None);
    let item_pause = MenuItem::new("Pause auto-switch", true, None);
    let item_about = MenuItem::new(
        format!("About {APP_NAME} v{}", env!("CARGO_PKG_VERSION")),
        false,
        None,
    );
    let item_quit = MenuItem::new("Quit", true, None);
    menu.append_items(&[
        &item_settings_ui,
        &item_settings_file,
        &item_logs,
        &item_wordlists,
        &item_layouts,
        &item_reload,
        &PredefinedMenuItem::separator(),
        &item_pause,
        &PredefinedMenuItem::separator(),
        &item_about,
        &item_quit,
    ])
    .context("populate tray menu")?;
    let settings_ui_id = item_settings_ui.id().clone();
    let settings_file_id = item_settings_file.id().clone();
    let logs_id = item_logs.id().clone();
    let wordlists_id = item_wordlists.id().clone();
    let layouts_id = item_layouts.id().clone();
    let reload_id = item_reload.id().clone();
    let pause_id = item_pause.id().clone();
    let quit_id = item_quit.id().clone();

    // Initial icon: query the OS for the current layout so we don't
    // flash a "??" before the first LayoutChanged event arrives.
    let initial_layout: Option<LayoutId> = layout_switcher.current().ok();
    let initial_icon = match initial_layout.as_ref() {
        Some(l) => icon_render::for_layout(l, false)?,
        None => icon_render::unknown()?,
    };

    let tray: TrayIcon = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(tooltip_for(initial_layout.as_ref(), false))
        .with_icon(initial_icon)
        .build()
        .context("build tray icon")?;

    // Cloned reference into the event loop so we can flip the menu
    // item's text between "⏸ Pause auto-switch" and "▶ Resume
    // auto-switch" when the engine reports a state change. MenuItem
    // is internally Arc-shared, so this clone bumps a refcount.
    let item_pause_for_loop = item_pause.clone();

    // Global hotkeys — strings come from `[hotkeys]` in config.toml.
    // We parse them with `global-hotkey`'s `FromStr` (see
    // `parse_hotkey_or_default`); on a malformed entry we fall back to
    // the documented default so the user never ends up with a tray app
    // that silently lost its hotkeys after a typo.
    let hotkey_manager = GlobalHotKeyManager::new().context("create global-hotkey manager")?;
    let hk_pause = parse_hotkey_or_default(
        &settings.snapshot().hotkeys.pause_toggle,
        "Ctrl+Shift+Space",
    );
    let hk_switch = parse_hotkey_or_default(
        &settings.snapshot().hotkeys.manual_switch_last,
        "Ctrl+Shift+Backspace",
    );
    if let Err(e) = hotkey_manager.register(hk_pause) {
        warn!(?e, hotkey = ?hk_pause, "could not register pause hotkey");
    }
    if let Err(e) = hotkey_manager.register(hk_switch) {
        warn!(?e, hotkey = ?hk_switch, "could not register switch-last hotkey");
    }
    let pause_hotkey_id = hk_pause.id();
    let switch_hotkey_id = hk_switch.id();

    // User-defined "smart commands" (text triggers like `anrl ` →
    // `Anatomical Reference List`) are NOT registered as global
    // hotkeys — they're consulted by the engine on every word
    // boundary. See `kb_core::commands` for the architecture and
    // `SwitcherEngine::decide` for the dispatch path.

    spawn_event_bridges(event_loop.create_proxy(), engine_event_rx.clone())?;

    // Layout poller: the engine emits LayoutChanged for switches it
    // performs itself, but we miss user-driven manual switches (Win+
    // Space / Alt+Shift / language bar / ibus / kde-keyboard). Polling
    // the OS-level current-layout query every ~250 ms catches those
    // cheaply and keeps the tray icon in sync.
    spawn_layout_poller(Arc::clone(&layout_switcher), engine_event_tx_for_poller)?;

    // Focus-driven wordlist profile watcher: same cadence as the
    // layout poller. Cheap when no profiles are configured (the
    // profile-cache HashMap is empty so the swap path is a no-op).
    if !profile_dict_cache.read().is_empty() {
        spawn_profile_watcher(
            Arc::clone(&focus_tracker),
            Arc::clone(&settings),
            Arc::clone(&profile_dict_cache),
            Arc::clone(&profile_force_reapply),
            dict_reload_handle.handle(),
        )?;
    }

    let settings_path: PathBuf = settings.path().to_owned();
    let log_dir: Option<PathBuf> = SettingsStore::log_dir().ok();
    let cmd_tx_for_loop = engine_cmd_tx.clone();
    let settings_for_loop = Arc::clone(&settings);

    // Tray-side mirror of engine state. Updated from PausedChanged
    // and LayoutChanged events; consulted whenever we need to redraw
    // (icon + tooltip both depend on both fields).
    let mut tray_state = TrayState {
        layout: initial_layout,
        paused: false,
    };

    info!("entering event loop");
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(UserEvent::Menu(id)) => {
                if id == quit_id {
                    info!("Quit clicked — shutting down");
                    if let Some(mut listener) = input_listener.take() {
                        listener.stop();
                    }
                    *control_flow = ControlFlow::Exit;
                } else if id == settings_ui_id {
                    spawn_settings_ui(SettingsCloseDeps {
                        settings: Arc::clone(&settings_for_loop),
                        layouts: Arc::clone(&layouts),
                        data_dir: data_dir.clone(),
                        user_wordlist_dir: user_wordlist_dir.clone(),
                        dict_reload_handle: dict_reload_handle.handle(),
                        profile_dict_cache: Arc::clone(&profile_dict_cache),
                        profile_force_reapply: Arc::clone(&profile_force_reapply),
                        reload_tx: cmd_tx_for_loop.clone(),
                    });
                } else if id == settings_file_id {
                    open_path(&settings_path, "settings file");
                } else if id == logs_id {
                    if let Some(dir) = log_dir.as_ref() {
                        let _ = std::fs::create_dir_all(dir);
                        open_path(dir, "log directory");
                    } else {
                        warn!("log directory unknown");
                    }
                } else if id == wordlists_id {
                    // First-run: the directory typically doesn't
                    // exist yet — ensure_user_wordlist_dir creates
                    // it (and seeds a tiny README so the user knows
                    // what files are recognised) before we open it.
                    match ensure_user_wordlist_dir() {
                        Ok(dir) => open_path(&dir, "user wordlists folder"),
                        Err(e) => warn!(?e, "could not prepare user wordlists folder"),
                    }
                } else if id == layouts_id {
                    // Same first-run treatment as wordlists: ensure
                    // the directory exists and drop a README that
                    // explains the TOML schema so the user can copy
                    // an embedded mapping from the repo as a starting
                    // point. New layouts in this folder are picked up
                    // on app restart.
                    match ensure_user_layout_dir() {
                        Ok(dir) => open_path(&dir, "user layouts folder"),
                        Err(e) => warn!(?e, "could not prepare user layouts folder"),
                    }
                } else if id == reload_id {
                    // Reload `config.toml` AND re-read user-overlay
                    // wordlists (`<config-dir>/wordlists/<stem>.txt`).
                    // The latter is what lets users add tech vocab
                    // like `kubectl` / `terraform` and have it pick
                    // up without restarting the app.
                    let reloaded_dicts = reload_user_dictionaries(&dict_reload_handle);
                    match settings_for_loop.reload() {
                        Ok(changed) => {
                            info!(
                                config_changed = changed,
                                dicts_reloaded = reloaded_dicts,
                                "Reload Settings"
                            );
                            if changed {
                                let _ = cmd_tx_for_loop.send(EngineCommand::SettingsReloaded);
                            }
                        }
                        Err(e) => warn!(?e, "could not reload config.toml"),
                    }
                } else if id == pause_id {
                    let _ = cmd_tx_for_loop.send(EngineCommand::TogglePause);
                }
            }
            Event::UserEvent(UserEvent::Hotkey(id)) => {
                if id == pause_hotkey_id {
                    let _ = cmd_tx_for_loop.send(EngineCommand::TogglePause);
                } else if id == switch_hotkey_id {
                    let _ = cmd_tx_for_loop.send(EngineCommand::SwitchLastForcefully);
                }
            }
            Event::UserEvent(UserEvent::Engine(ev)) => {
                handle_engine_event(
                    ev,
                    &tray,
                    &item_pause_for_loop,
                    &mut tray_state,
                    &settings_for_loop,
                    &layouts,
                );
            }
            _ => {}
        }
    });
}

/// Snapshot of "what should the tray look like right now". We need
/// both fields to render the icon / tooltip correctly — paused state
/// affects styling regardless of layout, and vice versa — so we
/// redraw from the whole struct on every relevant event.
struct TrayState {
    layout: Option<LayoutId>,
    paused: bool,
}

fn tooltip_for(layout: Option<&LayoutId>, paused: bool) -> String {
    match (layout, paused) {
        (Some(l), false) => format!("{APP_NAME} — {l}"),
        (Some(l), true) => format!("{APP_NAME} — {l} (paused)"),
        (None, false) => APP_NAME.to_owned(),
        (None, true) => format!("{APP_NAME} (paused)"),
    }
}

/// Redraw icon + tooltip + the pause menu-item text from the current
/// `TrayState`. Cheap (no allocation in the icon-rendering path beyond
/// a 16x16 RGBA buffer); safe to call on every state change.
fn refresh_tray(tray: &TrayIcon, item_pause: &MenuItem, state: &TrayState) {
    let icon_result = match state.layout.as_ref() {
        Some(l) => icon_render::for_layout(l, state.paused),
        None => icon_render::unknown(),
    };
    match icon_result {
        Ok(icon) => {
            if let Err(e) = tray.set_icon(Some(icon)) {
                warn!(?e, "could not update tray icon");
            }
        }
        Err(e) => warn!(?e, "could not render tray icon"),
    }
    if let Err(e) = tray.set_tooltip(Some(tooltip_for(state.layout.as_ref(), state.paused))) {
        warn!(?e, "could not update tray tooltip");
    }
    item_pause.set_text(if state.paused {
        "▶ Resume auto-switch"
    } else {
        "⏸ Pause auto-switch"
    });
}

fn open_path(path: &std::path::Path, what: &str) {
    debug!(?path, "opening {what}");
    if let Err(e) = opener::open(path) {
        warn!(?e, ?path, "could not open {what} in default app");
    }
}

/// Parse a hotkey string from `[hotkeys]` (e.g. `"Ctrl+Shift+Space"`)
/// using `global-hotkey`'s native `FromStr`. On parse failure we log
/// a warning and fall back to `default_str` so the app boots with a
/// usable hotkey rather than nothing — matches the Settings UI's
/// "loud-but-graceful" approach to bad config values.
fn parse_hotkey_or_default(s: &str, default_str: &str) -> HotKey {
    match s.parse::<HotKey>() {
        Ok(h) => h,
        Err(e) => {
            warn!(
                ?e,
                raw = s,
                fallback = default_str,
                "could not parse hotkey; using fallback"
            );
            // The fallback is itself a parse — built from a known-good
            // literal. If even that fails we hard-code the matching
            // (Ctrl+Shift)+key combo so we always return a real hotkey.
            default_str
                .parse::<HotKey>()
                .unwrap_or_else(|_| HotKey::new(Some(HkMods::CONTROL | HkMods::SHIFT), Code::Space))
        }
    }
}

/// Build one dictionary set per configured wordlist profile, ready
/// to swap into [`kb_detect::DictionaryDetector`] when focus enters
/// the matching app(s). Empty `wordlists.profiles` → empty cache;
/// the focus watcher is then never spawned, so this is zero-cost
/// for the common no-profile case.
///
/// Each profile reuses the bundled FSTs through the `Arc` inside
/// `LayoutDictionary` — only the user-overlay HashSets are
/// re-derived. So building 5 profiles takes 5 × (number-of-layouts)
/// disk-cheap text-file reads, not 5 × FST decode.
fn build_profile_dictionary_cache(
    layouts: &Arc<LayoutDb>,
    data_dir: &std::path::Path,
    wordlists: &WordlistSettings,
) -> HashMap<String, HashMap<LayoutId, kb_detect::LayoutDictionary>> {
    let mut out: HashMap<String, HashMap<LayoutId, kb_detect::LayoutDictionary>> = HashMap::new();
    for profile in &wordlists.profiles {
        let Some(dir) = kb_core::layouts::user_profile_wordlist_dir(&profile.id) else {
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

/// Type alias for the shared profile dictionary cache. Behind an
/// `Arc<RwLock<...>>` so the close-handler in `spawn_settings_ui`
/// can rebuild it from disk after the user saves wordlist edits via
/// the GUI. Watcher takes a read lock per tick; rebuilds (rare —
/// only on Settings UI close) take a write lock briefly.
type ProfileDictCache =
    Arc<RwLock<HashMap<String, HashMap<LayoutId, kb_detect::LayoutDictionary>>>>;

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
fn spawn_profile_watcher(
    focus_tracker: Arc<dyn kb_input::FocusTracker>,
    settings: Arc<SettingsStore>,
    profile_cache: ProfileDictCache,
    force_reapply: Arc<AtomicBool>,
    dict_handle: kb_detect::DictionaryDetector,
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
fn build_full_profile_cache(
    layouts: &Arc<LayoutDb>,
    data_dir: &Path,
    wordlists: &WordlistSettings,
    user_wordlist_dir: Option<&Path>,
) -> HashMap<String, HashMap<LayoutId, kb_detect::LayoutDictionary>> {
    let mut cache = build_profile_dictionary_cache(layouts, data_dir, wordlists);
    if !cache.is_empty() {
        let global = layouts
            .build_profile_dictionaries(data_dir, user_wordlist_dir.unwrap_or(Path::new("")));
        cache.insert(String::new(), global);
    }
    cache
}

/// CLI help text. Kept short and stable — most users never invoke
/// kb-switcher with arguments, but `--help` should still answer the
/// "what does this thing do" question without a manpage.
fn print_help() {
    println!(
        "{APP_NAME} {ver}\n\
        \n\
        USAGE:\n  \
            kb-switcher              start the tray app\n  \
            kb-switcher --settings   open the settings window\n  \
            kb-switcher --version    print version and exit\n  \
            kb-switcher --help       show this help",
        ver = env!("CARGO_PKG_VERSION"),
    );
}

/// Bag of dependencies the settings-UI close handler needs to do
/// the full reload (config.toml + global wordlists + per-profile
/// cache + force-reapply on the watcher). Grouped as a struct so
/// the call site at the menu handler isn't a wall of args.
struct SettingsCloseDeps {
    settings: Arc<SettingsStore>,
    layouts: Arc<LayoutDb>,
    data_dir: PathBuf,
    user_wordlist_dir: Option<PathBuf>,
    dict_reload_handle: kb_detect::DictionaryDetector,
    profile_dict_cache: ProfileDictCache,
    profile_force_reapply: Arc<AtomicBool>,
    reload_tx: Sender<EngineCommand>,
}

/// Spawn the Settings GUI as a child process (`kb-switcher
/// --settings`) and refresh the engine when the window closes.
/// Subprocess instead of in-process for the macOS main-thread
/// reason documented at the top of `settings_ui.rs`.
///
/// What "refresh" means in practice — we run all three on close so
/// every kind of edit the user could have made via the GUI takes
/// effect by the time the focus returns to their app:
///
/// 1. **`config.toml` reload** — picks up `[[commands]]` text-trigger
///    entries, hotkey rebindings, exception list edits, profile
///    schema changes.
/// 2. **Global wordlist reload** — `<config-dir>/wordlists/<stem>.txt`
///    re-read; the engine's dictionary set swapped via
///    `DictionaryDetector::replace_dicts` (same primitive the tray
///    "Reload Settings" entry uses).
/// 3. **Per-profile wordlist cache rebuild + force-reapply** — the
///    profile dictionary cache is rebuilt from disk, and we set the
///    watcher's `force_reapply` flag so the *currently active*
///    profile's freshly-loaded dicts get re-applied on the next tick
///    (~250 ms). Without this, the watcher only swaps on profile
///    transitions, so a user editing words while focused on a
///    profiled app would see no effect until they alt-tabbed away
///    and back.
///
/// Best-effort: if we can't even locate our own exe (highly unusual,
/// e.g. running from a deleted binary) we log + skip rather than
/// taking down the tray.
fn spawn_settings_ui(deps: SettingsCloseDeps) {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            warn!(?e, "could not locate own exe; can't open settings UI");
            return;
        }
    };
    info!(?exe, "launching settings UI");
    let child = match std::process::Command::new(&exe).arg("--settings").spawn() {
        Ok(c) => c,
        Err(e) => {
            warn!(?e, ?exe, "settings UI subprocess failed to start");
            return;
        }
    };

    // Wait for the child in a worker thread so the tray doesn't
    // block. On exit we run the three refresh steps documented on
    // the function. We do all three regardless of whether the user
    // clicked Save — the GUI also has an "Open config.toml" button
    // and a Wordlists pane Save that writes files outside the
    // GUI's own state, so reload-on-close gives the most
    // predictable contract: "everything you did in the GUI applies
    // now."
    std::thread::Builder::new()
        .name("kb-switcher-settings-waiter".into())
        .spawn(move || {
            let mut child = child;
            match child.wait() {
                Ok(status) => info!(?status, "settings UI exited"),
                Err(e) => warn!(?e, "could not wait on settings UI child"),
            }

            // (1) config.toml reload.
            match deps.settings.reload() {
                Ok(changed) => info!(changed, "config.toml reloaded after settings UI exit"),
                Err(e) => warn!(?e, "could not reload config.toml after settings UI exit"),
            }

            // (2) Global wordlist reload — same path as the tray
            // "Reload Settings" menu entry.
            let n = reload_user_dictionaries(&deps.dict_reload_handle);
            info!(
                loaded = n,
                "wordlist dictionaries reloaded after settings UI exit"
            );

            // (3) Profile cache rebuild + watcher force-reapply.
            // Rebuild always, even when the user has no profiles
            // configured — cheap (empty cache) and keeps the
            // contract uniform.
            let snap = deps.settings.snapshot();
            let fresh_cache = build_full_profile_cache(
                &deps.layouts,
                &deps.data_dir,
                &snap.wordlists,
                deps.user_wordlist_dir.as_deref(),
            );
            let n_profiles = fresh_cache.len();
            *deps.profile_dict_cache.write() = fresh_cache;
            deps.profile_force_reapply.store(true, Ordering::Release);
            info!(
                profiles = n_profiles,
                "profile cache rebuilt; watcher will re-apply on next tick"
            );

            // (4) Tell the engine to clear its word buffer + refresh
            // audio for the new settings snapshot. Sent last so any
            // observer sees the rebuilds before the engine command.
            if let Err(e) = deps.reload_tx.send(EngineCommand::SettingsReloaded) {
                warn!(?e, "could not enqueue SettingsReloaded after UI exit");
            }
        })
        .ok();
}

/// Resolve the user wordlists directory, create it if missing, and
/// drop a `README.txt` on first creation so a user opening the
/// folder for the first time can immediately see what files are
/// recognised. Returns the directory path on success.
///
/// We seed only on actual creation — once the user has the folder,
/// we never touch the README again, so users can delete it / rename
/// it / replace it without our re-overwriting their changes.
fn ensure_user_wordlist_dir() -> anyhow::Result<PathBuf> {
    let dir = kb_core::layouts::user_wordlist_dir()
        .context("could not determine user-config directory")?;
    let needs_seed = !dir.exists();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("could not create wordlists dir at {}", dir.display()))?;
    if needs_seed {
        let readme = dir.join("README.txt");
        // Best-effort write — failure is logged but doesn't block
        // opening the folder. The directory itself is the value.
        if let Err(e) = std::fs::write(&readme, USER_WORDLISTS_README) {
            warn!(?e, ?readme, "could not seed README in wordlists folder");
        }
    }
    Ok(dir)
}

/// Resolve the user layouts directory, create it if missing, and
/// drop a `README.txt` on first creation so a user opening it for
/// the first time can immediately see the TOML schema and pick up
/// an embedded mapping as a starting point. Returns the directory
/// path on success.
///
/// Same single-shot behaviour as the wordlists README — once the
/// directory exists we never touch the README again.
fn ensure_user_layout_dir() -> anyhow::Result<PathBuf> {
    let dir =
        kb_core::layouts::user_layout_dir().context("could not determine user-config directory")?;
    let needs_seed = !dir.exists();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("could not create layouts dir at {}", dir.display()))?;
    if needs_seed {
        let readme = dir.join("README.txt");
        if let Err(e) = std::fs::write(&readme, USER_LAYOUTS_README) {
            warn!(?e, ?readme, "could not seed README in layouts folder");
        }
    }
    Ok(dir)
}

/// One-time README seeded into the user layouts folder. Mirrors the
/// wordlists README's plain-text, no-markdown style.
const USER_LAYOUTS_README: &str = "\
kb-switcher — user layouts
===========================

Drop layout-mapping TOML files here to add support for keyboards /
languages the app doesn't ship out of the box. New layouts are
picked up on the next app start.

File naming:
    Use a clear file stem matching the language code, lowercase, with
    underscore between language and country: `pl_pl.toml`, `tr_tr.toml`,
    `cs_cz.toml`, `nl_nl.toml`, …

TOML schema (same as the bundled `data/layout-mappings/*.toml`):

    id     = \"pl-PL\"          # BCP-47 ish; what config.toml refers to
    name   = \"Polski\"         # display name in the tray (optional)
    script = \"Latin\"          # Latin / Cyrillic / Greek / Armenian / Hebrew / Arabic / Other

    [keys]
    # Win SC Set-1 scancode → produced character.
    # `plain` is unshifted, `shift` is the shifted variant (optional).
    0x10 = { plain = \"q\", shift = \"Q\" }
    0x11 = { plain = \"w\", shift = \"W\" }
    # … and so on for the alphanumeric / punctuation rows that
    #   matter for word-boundary detection.

The bundled `en_us.toml` and `uk_ua.toml` files are excellent
copy-paste starting points — see the kb-switcher source repo,
`data/layout-mappings/`.

Picking up dictionary support:
    To get full word-detection (not just plausibility scoring),
    drop matching wordlists alongside in
    `<config-dir>/kb-switcher/wordlists/`:

        <stem>.txt          # main wordlist, one lowercase word per line
        <stem>-extras.txt   # same effect, separate file for organisation
        <stem>-stop.txt     # 1- and 2-letter stop words

    where `<stem>` is your TOML file's stem (`pl_pl` for `pl_pl.toml`).
    See the user wordlists README in `<config-dir>/kb-switcher/wordlists/`
    for the format.

Override the bundled mapping:
    If your TOML's `id` matches an embedded layout (e.g. `de-DE`),
    your file wins. Use this if your physical keyboard differs from
    the bundled mapping.
";

/// One-time README seeded into the user wordlists folder. Plain
/// text (no markdown), short, and readable in any editor / preview
/// pane. Matches the file conventions documented in
/// `kb_core::layouts::build_dictionary`.
const USER_WORDLISTS_README: &str = "\
kb-switcher — user wordlists
=============================

Drop text files here to extend the built-in dictionaries without
rebuilding the app. Changes are picked up on the next \"Reload
Settings\" tray click (Ctrl+Shift+R if you've bound it) — no restart
needed.

Per layout, three filenames are recognised. Replace `<stem>` with the
layout id you want to extend (`en_us`, `uk_ua`, …):

    <stem>.txt          One word per line; treated as a real word
                        in this layout, regardless of length.
                        Use this for tech vocab, surnames, slang,
                        product names — anything that should NOT
                        get auto-corrected away.

    <stem>-extras.txt   Same effect as <stem>.txt; separate file
                        so you can organise (e.g. one for tech
                        vocab, one for personal names). Both are
                        merged into the same overlay at load time.

    <stem>-stop.txt     Curated 1- and 2-letter additions. Needed
                        when you want a SHORT (≤2 letter) token
                        treated as a real word — at that length
                        the embedded full dictionary is bypassed
                        on purpose, so this is the only path that
                        works for short tokens.

Format for all three:
    - one lowercase word per line
    - blank lines and `# comment` lines ignored
    - UTF-8

Example (`uk_ua.txt`):
    кубернетес
    докерфайл
    редіс

Example (`uk_ua-stop.txt`):
    хм
    тю

Tip: the embedded dictionaries already cover ~370k EN and ~333k UK
entries plus a curated tech-vocab list. You only need files here for
words you actually see auto-corrected wrongly.
";

fn handle_engine_event(
    ev: SwitcherEvent,
    tray: &TrayIcon,
    item_pause: &MenuItem,
    state: &mut TrayState,
    settings: &Arc<SettingsStore>,
    layouts: &Arc<LayoutDb>,
) {
    match ev {
        SwitcherEvent::Corrected {
            from_layout,
            to_layout,
            original_text,
            corrected_text,
            reason,
        } => {
            info!(
                %from_layout,
                %to_layout,
                original = %original_text,
                corrected = %corrected_text,
                %reason,
                "correction applied"
            );
            // System notification — the user explicitly opted into
            // these via `[general].show_notifications`. We never log
            // the actual typed text in the notification body (per
            // CLAUDE.md: never log user-typed text); the notification
            // shows only the layout transition, which is the useful
            // "what just happened" signal.
            if settings.snapshot().general.show_notifications {
                spawn_layout_change_notification(layouts, &to_layout);
            }
        }
        SwitcherEvent::PausedChanged(paused) => {
            info!(paused, "engine paused state changed");
            state.paused = paused;
            refresh_tray(tray, item_pause, state);
        }
        SwitcherEvent::LayoutChanged(id) => {
            debug!(layout = %id, "layout changed");
            state.layout = Some(id);
            refresh_tray(tray, item_pause, state);
        }
        SwitcherEvent::KeptCurrent { reason } => {
            debug!(%reason, "decision: keep current");
        }
    }
}

/// Show a 2-second toast / notification that the engine just
/// auto-switched to a new layout. Spawned on a worker thread because
/// `notify-rust::Notification::show()` is synchronous and the time it
/// takes varies per platform (DBus round-trip on Linux, NSUserNotification
/// on macOS, Toast XML on Windows) — we don't want to add even a few ms
/// to the tray's event-loop latency for a cosmetic side effect.
///
/// Failures are logged at warn level and swallowed: a missing notification
/// daemon (Linux dev container, macOS sandbox quirks, Windows Focus
/// Assist suppressing toasts) shouldn't propagate up to the tray. The
/// auto-switch itself already happened — the notification is just the
/// optional UX sugar layer on top.
fn spawn_layout_change_notification(layouts: &Arc<LayoutDb>, to_layout: &LayoutId) {
    // Resolve the layout's display `name` if we have a mapping for it
    // (`English (United States)` for `en-US`, etc.). Falls back to the
    // raw BCP-47 id otherwise — never a panic, never a stale string.
    let pretty = layouts
        .get(to_layout)
        .map(|m| m.name.clone())
        .unwrap_or_else(|| to_layout.as_str().to_owned());

    let to_owned = to_layout.as_str().to_owned();
    std::thread::Builder::new()
        .name("kb-switcher-notify".into())
        .spawn(move || {
            let mut n = notify_rust::Notification::new();
            n.summary("kb-switcher")
                .body(&format!("Switched to {pretty}"))
                .appname(APP_NAME)
                .timeout(notify_rust::Timeout::Milliseconds(2000));
            // `icon` is best-effort — passing an OS-specific identifier
            // works on Linux/Windows when a matching theme icon exists,
            // and is silently ignored otherwise. We don't ship our own
            // installed icon yet, so leave it out and let the platform's
            // default app-notification glyph render.
            if let Err(e) = n.show() {
                warn!(?e, layout = %to_owned, "could not show layout-change notification");
            }
        })
        .ok();
}

fn spawn_event_bridges(
    proxy: EventLoopProxy<UserEvent>,
    engine_rx: Receiver<SwitcherEvent>,
) -> Result<()> {
    let proxy_menu = proxy.clone();
    std::thread::Builder::new()
        .name("tray-menu-bridge".into())
        .spawn(move || {
            let rx = MenuEvent::receiver();
            while let Ok(ev) = rx.recv() {
                if proxy_menu.send_event(UserEvent::Menu(ev.id)).is_err() {
                    break;
                }
            }
        })
        .context("spawn menu bridge thread")?;

    let proxy_hk = proxy.clone();
    std::thread::Builder::new()
        .name("hotkey-bridge".into())
        .spawn(move || {
            let rx = GlobalHotKeyEvent::receiver();
            while let Ok(ev) = rx.recv() {
                // global-hotkey 0.6+ emits BOTH `Pressed` and
                // `Released` events for the same chord. Forwarding
                // both meant the pause-toggle handler ran twice per
                // user keypress — net effect: pause flipped on
                // press, then immediately back on release, so the
                // user only saw "paused" while physically holding
                // the chord. Filter to Pressed only.
                if ev.state != HotKeyState::Pressed {
                    continue;
                }
                if proxy_hk.send_event(UserEvent::Hotkey(ev.id)).is_err() {
                    break;
                }
            }
        })
        .context("spawn hotkey bridge thread")?;

    std::thread::Builder::new()
        .name("engine-event-bridge".into())
        .spawn(move || {
            for ev in engine_rx.iter() {
                if proxy.send_event(UserEvent::Engine(ev)).is_err() {
                    break;
                }
            }
        })
        .context("spawn engine event bridge thread")?;

    Ok(())
}

fn build_plausibility_detector(layouts: &Arc<LayoutDb>) -> WordPlausibilityDetector {
    let profiles = layouts
        .iter()
        .map(|(id, m)| (id.clone(), m.detector_profile()))
        .collect();
    WordPlausibilityDetector::new(profiles)
}

fn build_dictionary_detector(layouts: &Arc<LayoutDb>) -> DictionaryDetector {
    DictionaryDetector::new(collect_dicts(layouts))
}

fn collect_dicts(
    layouts: &LayoutDb,
) -> std::collections::HashMap<kb_types::LayoutId, kb_detect::LayoutDictionary> {
    layouts
        .iter()
        .filter_map(|(id, m)| m.dictionary.as_ref().map(|d| (id.clone(), d.clone())))
        .collect()
}

/// Re-read `<config-dir>/kb-switcher/wordlists/<stem>.txt` from disk
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
///   `<config-dir>/kb-switcher/layouts/`) → require an app restart.
///   The engine holds a snapshot `Arc<LayoutDb>`, so the new layout
///   wouldn't be in its scancode-translation tables anyway. We log
///   loud-and-clear if we see one, so the user knows.
/// * **Per-profile wordlist overlays**
///   (`<config-dir>/kb-switcher/wordlists/profiles/<id>/<stem>.txt`)
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
fn reload_user_dictionaries(handle: &DictionaryDetector) -> usize {
    let wordlist_dir = kb_core::layouts::user_wordlist_dir();
    let layout_dir = kb_core::layouts::user_layout_dir();
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

/// Poll the OS for the current layout every ~250 ms; emit a
/// `LayoutChanged` event whenever the answer differs from last time.
/// Catches manual switches done outside our engine (language bar,
/// Win+Space, Alt+Shift, ibus / kde keyboard, …) so the tray icon
/// stays in sync.
fn spawn_layout_poller(
    switcher: Arc<dyn kb_layout::LayoutSwitcher>,
    out_tx: crossbeam_channel::Sender<SwitcherEvent>,
) -> Result<()> {
    std::thread::Builder::new()
        .name("kb-layout-poller".into())
        .spawn(move || {
            let mut last: Option<LayoutId> = None;
            loop {
                if let Ok(current) = switcher.current() {
                    if last.as_ref() != Some(&current) {
                        if out_tx
                            .send(SwitcherEvent::LayoutChanged(current.clone()))
                            .is_err()
                        {
                            break;
                        }
                        last = Some(current);
                    }
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        })
        .context("spawn layout poller thread")?;
    Ok(())
}

/// Init `tracing` with both a stderr layer and a file appender that
/// rotates daily under `<data_dir>/kb-switcher/logs/`. Returns the
/// guard for the file writer; dropping it would close the file.
fn init_tracing() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::{EnvFilter, fmt};

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let stderr_layer = fmt::layer().with_writer(std::io::stderr).with_target(false);

    let (file_layer, guard) = match SettingsStore::log_dir() {
        Ok(dir) => {
            if let Err(e) = std::fs::create_dir_all(&dir) {
                eprintln!("kb-switcher: could not create log dir {dir:?}: {e}");
                (None, None)
            } else {
                let appender = tracing_appender::rolling::daily(&dir, "kb-switcher.log");
                let (writer, guard) = tracing_appender::non_blocking(appender);
                let layer = fmt::layer()
                    .with_writer(writer)
                    .with_ansi(false)
                    .with_target(false);
                (Some(layer), Some(guard))
            }
        }
        Err(e) => {
            eprintln!("kb-switcher: cannot resolve log dir: {e}");
            (None, None)
        }
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer)
        .with(file_layer)
        .init();

    guard
}

// ─── Noop key emitter (graceful fallback on unimplemented platforms) ──

struct NoopEmitter;

impl kb_input::KeyEmitter for NoopEmitter {
    fn send_backspaces(&self, n: usize) -> Result<(), kb_input::InputError> {
        debug!(n, "noop emitter: would send backspaces");
        Ok(())
    }
    fn send_text(&self, text: &str) -> Result<(), kb_input::InputError> {
        debug!(text, "noop emitter: would send text");
        Ok(())
    }
    fn backend_name(&self) -> &'static str {
        "noop"
    }
}

fn noop_emitter() -> Box<dyn kb_input::KeyEmitter> {
    Box::new(NoopEmitter)
}
