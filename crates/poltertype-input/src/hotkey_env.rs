//! How global hotkeys behave on this machine, answered without
//! building a backend.
//!
//! The Settings window is a separate process: it has no listener, no
//! layout switcher, and no way to ask the tray what it decided. It
//! still has to show the chord the tray is really listening for.
//! Issue #31 is what happens otherwise — the tray rebound the
//! force-switch chord on Wayland, Settings went on displaying the
//! configured default, and the user pressed what the window said and
//! got nothing.
//!
//! Deliberately phrased as *facts about the machine* rather than
//! backend names: what a caller needs to know is why a chord is
//! unusable here, not which module answers.

/// The two properties of this session that can make a default hotkey
/// the wrong choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HotkeyEnvironment {
    /// The chord is *observed* on the key stream rather than consumed
    /// by the OS, so it reaches the focused application as well.
    /// Anything destructive there is unusable — `Ctrl+Backspace`
    /// deletes the very word a force-switch is about to fix. True on
    /// the Wayland/evdev backend.
    pub observed_not_consumed: bool,
    /// The OS already owns `Ctrl+Shift+Space`: macOS's "select the
    /// previous input source". Claiming it globally would preempt the
    /// layout switching this app exists to complement.
    pub system_owns_ctrl_shift_space: bool,
}

/// Probe the current session. Cheap — an environment-variable read at
/// most — so callers may treat it as free.
pub fn hotkey_environment() -> HotkeyEnvironment {
    HotkeyEnvironment {
        #[cfg(target_os = "linux")]
        observed_not_consumed: crate::linux::session_kind() != crate::linux::SessionKind::X11,
        #[cfg(not(target_os = "linux"))]
        observed_not_consumed: false,

        system_owns_ctrl_shift_space: cfg!(target_os = "macos"),
    }
}
