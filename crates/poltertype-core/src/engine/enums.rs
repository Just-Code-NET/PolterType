//! Engine message enums: outbound notifications and inbound commands.

use poltertype_layout::LayoutId;

use super::types::KeystreamHotkeys;

/// Outbound notifications the engine emits.
#[derive(Debug, Clone)]
pub enum SwitcherEvent {
    /// Layout (silently) switched — useful for the tray icon to update.
    LayoutChanged(LayoutId),
    /// A correction has just been applied.
    Corrected {
        from_layout: LayoutId,
        to_layout: LayoutId,
        original_text: String,
        corrected_text: String,
        reason: String,
    },
    /// Engine has been paused / resumed via hotkey.
    PausedChanged(bool),
    /// Engine looked at the buffer but decided to keep the current
    /// layout — useful for debug overlays.
    KeptCurrent { reason: String },
}

/// Commands sent into the engine from the app loop.
#[derive(Debug, Clone)]
pub enum EngineCommand {
    /// Toggle paused state (Pause-hotkey).
    TogglePause,
    /// Force a switch on the most recently completed word, ignoring
    /// the detector (Manual-switch-last hotkey).
    SwitchLastForcefully,
    /// Settings changed; refresh whatever caches the engine keeps.
    SettingsReloaded,
    /// Enable (or update) hotkey detection straight off the key stream.
    /// Used on backends where the OS-level [`global-hotkey`] grab can't
    /// see input — notably Wayland, where the evdev listener is the only
    /// thing that observes `Ctrl+Shift+Space` at all. An empty value
    /// disables keystream detection (the OS grab is doing the job).
    SetKeystreamHotkeys(KeystreamHotkeys),
}

pub enum Either<A, B> {
    Cmd(A),
    Key(B),
}
