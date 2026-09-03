//! What [`SwitcherEngine::new`](super::engine::SwitcherEngine::new) is built out of.

use std::sync::Arc;

use crossbeam_channel::Sender;
use poltertype_detect::{Detector, SuggestionProvider};
use poltertype_input::{Clipboard, FocusTracker, KeyEmitter, KeyGate};
use poltertype_layout::LayoutSwitcher;

use crate::audio::AudioPlayer;
use crate::engine::enums::SwitcherEvent;
use crate::layouts::LayoutDb;
use crate::settings::SettingsStore;

/// Everything the engine is built out of.
///
/// A struct rather than positional parameters because seven of these are
/// `Arc<dyn …>` trait objects: any two of the same shape transpose at
/// the call site and still compile. Named fields make the wiring in
/// `main.rs` impossible to get wrong that way.
pub struct EngineDeps {
    pub settings: Arc<SettingsStore>,
    pub layouts: Arc<LayoutDb>,
    pub detectors: Vec<Box<dyn Detector>>,
    pub layout_switcher: Arc<dyn LayoutSwitcher>,
    pub key_emitter: Arc<dyn KeyEmitter>,
    /// `None` where the session offers no windowless clipboard access,
    /// which turns selection conversion off however the setting reads.
    pub clipboard: Option<Arc<dyn Clipboard>>,
    pub key_gate: KeyGate,
    pub focus_tracker: Arc<dyn FocusTracker>,
    pub audio: Arc<AudioPlayer>,
    pub out_tx: Sender<SwitcherEvent>,
    /// `None` when no suggestion provider is wired — the feature is
    /// then inert, not merely disabled.
    pub suggester: Option<Arc<dyn SuggestionProvider>>,
}
