//! Plain data types shared across the binary.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use parking_lot::RwLock;

use anyhow::Result;
use crossbeam_channel::Sender;
use poltertype_core::engine::EngineCommand;
use poltertype_core::layouts::LayoutDb;
use poltertype_core::settings::SettingsStore;
use poltertype_types::LayoutId;
use tracing::debug;

/// Snapshot of "what should the tray look like right now". We need
/// all fields to render the icon / tooltip correctly — paused state
/// affects styling regardless of layout, and vice versa — so we
/// redraw from the whole struct on every relevant event.
pub(crate) struct TrayState {
    pub(crate) layout: Option<LayoutId>,
    pub(crate) paused: bool,
    /// Keyboard hooks failed to start. Fixed at startup: the only
    /// recovery is fixing permissions and relaunching, so this never
    /// flips back at runtime.
    pub(crate) input_alert: bool,
}

/// Type alias for the shared profile dictionary cache. Behind an
/// `Arc<RwLock<...>>` so the close-handler in `spawn_settings_ui`
/// can rebuild it from disk after the user saves wordlist edits via
/// the GUI. Watcher takes a read lock per tick; rebuilds (rare —
/// only on Settings UI close) take a write lock briefly.
pub(crate) type ProfileDictCache =
    Arc<RwLock<HashMap<String, HashMap<LayoutId, poltertype_detect::LayoutDictionary>>>>;

/// Bag of dependencies the settings-UI close handler needs to do
/// the full reload (config.toml + global wordlists + per-profile
/// cache + force-reapply on the watcher). Grouped as a struct so
/// the call site at the menu handler isn't a wall of args.
pub(crate) struct SettingsCloseDeps {
    pub(crate) settings: Arc<SettingsStore>,
    pub(crate) layouts: Arc<LayoutDb>,
    pub(crate) data_dir: PathBuf,
    pub(crate) user_wordlist_dir: Option<PathBuf>,
    pub(crate) dict_reload_handle: poltertype_detect::DictionaryDetector,
    pub(crate) profile_dict_cache: ProfileDictCache,
    pub(crate) profile_force_reapply: Arc<AtomicBool>,
    pub(crate) reload_tx: Sender<EngineCommand>,
}

pub(crate) struct NoopEmitter;

impl poltertype_input::KeyEmitter for NoopEmitter {
    fn send_backspaces(&self, n: usize) -> Result<(), poltertype_input::InputError> {
        debug!(n, "noop emitter: would send backspaces");
        Ok(())
    }
    fn send_text(&self, text: &str) -> Result<(), poltertype_input::InputError> {
        debug!(text, "noop emitter: would send text");
        Ok(())
    }
    fn backend_name(&self) -> &'static str {
        "noop"
    }
}

pub(crate) fn noop_emitter() -> Box<dyn poltertype_input::KeyEmitter> {
    Box::new(NoopEmitter)
}
