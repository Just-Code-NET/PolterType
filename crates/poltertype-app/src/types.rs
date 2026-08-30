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
use poltertype_core::settings::{SettingsStore, TrayIconStyle};
use poltertype_types::LayoutId;
use tao::event_loop::EventLoopProxy;
use tracing::debug;

/// Snapshot of "what should the tray look like right now". Icon and
/// tooltip each depend on more than one field, so a redraw always takes
/// the whole struct.
pub(crate) struct TrayState {
    pub(crate) layout: Option<LayoutId>,
    pub(crate) paused: bool,
    /// Keyboard hooks failed to start. Fixed at startup: the only
    /// recovery is fixing permissions and relaunching, so this never
    /// flips back at runtime.
    pub(crate) input_alert: bool,
    /// Which way round a `mono` icon has to read. Sampled once at
    /// startup: the probe shells out to a CLI tool, and the icon is
    /// redrawn on every layout change.
    pub(crate) polarity: crate::icon_render::PanelPolarity,
    /// How many things a plug-in is waiting on the user for — the count
    /// behind the mark on the tray icon. Zero draws nothing at all: the
    /// icon's job is to say what layout is in force.
    pub(crate) attention: u32,
    /// `[general].tray_icon`. Kept here so a redraw uses the same style
    /// the last config reload settled on, and so the event loop can see
    /// that the style changed at all.
    pub(crate) style: TrayIconStyle,
}

/// Words a tooltip offered "Add to dictionary" for and that went away
/// unused, newest first, so the tray can offer them again (issue #38).
///
/// **RAM only, and bounded.** This is the one place the app keeps words
/// the user typed beyond the engine's single-word buffer, so it holds
/// as few as are useful, never reaches a file, and never reaches a log
/// — `Debug` is deliberately not derived, to keep it out of one by
/// accident.
pub(crate) struct DeferredWords {
    words: Vec<(LayoutId, String)>,
}

impl DeferredWords {
    /// Enough to catch the ones that got away during a paragraph;
    /// short enough to stay a menu rather than a history.
    const CAP: usize = 8;

    pub(crate) fn new() -> Self {
        Self { words: Vec::new() }
    }

    /// Remember one, newest first. A repeat moves to the front rather
    /// than appearing twice — the same word missed again is the same
    /// word, and a duplicated row would read as two different offers.
    pub(crate) fn push(&mut self, layout: LayoutId, word: String) {
        if word.trim().is_empty() {
            return;
        }
        self.words.retain(|(l, w)| !(l == &layout && w == &word));
        self.words.insert(0, (layout, word));
        self.words.truncate(Self::CAP);
    }

    pub(crate) fn take(&mut self, layout: &LayoutId, word: &str) -> bool {
        let before = self.words.len();
        self.words.retain(|(l, w)| !(l == layout && w == word));
        self.words.len() != before
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &(LayoutId, String)> {
        self.words.iter()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.words.is_empty()
    }
}

/// The shared profile dictionary cache: the watcher takes a read lock per
/// tick, and the close-handler in `spawn_settings_ui` a brief write lock
/// to rebuild it from disk after the user saves wordlist edits.
pub(crate) type ProfileDictCache =
    Arc<RwLock<HashMap<String, HashMap<LayoutId, poltertype_detect::LayoutDictionary>>>>;

/// What the settings-UI close handler needs for the full reload:
/// config.toml, global wordlists, the per-profile cache, and
/// force-reapply on the watcher.
pub(crate) struct SettingsCloseDeps {
    pub(crate) settings: Arc<SettingsStore>,
    pub(crate) layouts: Arc<LayoutDb>,
    pub(crate) data_dir: PathBuf,
    pub(crate) user_wordlist_dir: Option<PathBuf>,
    pub(crate) dict_reload_handle: poltertype_detect::DictionaryDetector,
    pub(crate) profile_dict_cache: ProfileDictCache,
    pub(crate) profile_force_reapply: Arc<AtomicBool>,
    pub(crate) reload_tx: Sender<EngineCommand>,
    /// Announces the re-read to the tray, which owns the hotkey grabs.
    pub(crate) proxy: EventLoopProxy<crate::enums::UserEvent>,
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

#[cfg(test)]
mod tests;
