//! The two OS-hook extension points: listener and emitter.

use crate::*;
use crossbeam_channel::Sender;
pub use poltertype_types::KeyEvent;

/// A per-OS global keyboard listener.
///
/// Implementations must be `Send` so they can be moved onto a worker
/// thread. They are not required to be `Sync`; only one task drives the
/// listener at a time.
pub trait InputListener: Send {
    /// Start delivering events into `sink`. Returns once the OS hook
    /// is installed (or fails). The listener owns the worker thread
    /// for its lifetime.
    fn start(&mut self, sink: Sender<KeyEvent>) -> Result<(), InputError>;

    /// Stop and tear down the OS hook. Idempotent.
    fn stop(&mut self);

    /// Human-readable backend name (e.g. `"windows-ll-hook"`,
    /// `"linux-evdev"`). Useful for logs and the tray onboarding banner.
    fn backend_name(&self) -> &'static str;
}

/// Synthesises keystrokes — used by the corrector to delete the
/// just-typed word and re-type it after switching layouts.
///
/// All emitted events come back through [`InputListener`] with
/// `injected = true`; the engine drops those to avoid feedback.
pub trait KeyEmitter: Send + Sync {
    /// Emit `n` Backspace presses, one after another.
    fn send_backspaces(&self, n: usize) -> Result<(), InputError>;

    /// Emit `text` as Unicode characters. On Windows uses
    /// `KEYEVENTF_UNICODE`, which is layout-independent.
    fn send_text(&self, text: &str) -> Result<(), InputError>;

    /// Replay raw scancodes against whatever layout the OS is now in.
    ///
    /// The only correction path that works reliably on Wayland: the
    /// Unicode-compose trick `send_text` falls back to is silently
    /// swallowed — or typed literally — by most terminals and
    /// Wayland-native apps, whereas replaying the original scancodes
    /// after `switch_to` lets the compositor's xkb mapping produce the
    /// right glyphs.
    ///
    /// Platforms with a real Unicode-emit API override this to return
    /// `Unsupported`, so the engine falls back to `send_text`, which is
    /// more robust there.
    fn send_keys(&self, _keys: &[ReplayKey]) -> Result<(), InputError> {
        Err(InputError::Unsupported(
            "this backend has no scancode-replay path; use send_text".into(),
        ))
    }

    /// Release modifier keys the user is physically holding, before we
    /// type anything.
    ///
    /// Our injected keys travel the same path to the application as
    /// theirs, so a held `Ctrl` turns a replay into a burst of shortcuts
    /// and nothing is typed — exactly what happens when a correction is
    /// triggered *by* a chord.
    ///
    /// They are deliberately not pressed back: re-pressing a modifier
    /// the user has meanwhile let go of would leave it stuck down. Their
    /// own release lands on an already-up key and is ignored.
    ///
    /// Backends that cannot do this keep the default no-op — they just
    /// have the bug.
    fn release_modifiers(&self, _held: Modifiers) -> Result<(), InputError> {
        Ok(())
    }

    /// Drain the log of key events this emitter has synthesised since
    /// the last call. Backends whose events come back through the
    /// listener with a trustworthy `injected = true` flag (Windows,
    /// macOS) keep the default empty implementation — the engine's
    /// `injected` check already filters their echoes. The Linux uinput
    /// backend records every event so the engine can consume the
    /// untagged echoes off the key stream.
    fn take_emitted(&self) -> Vec<EmittedKey> {
        Vec::new()
    }

    fn backend_name(&self) -> &'static str;
}
