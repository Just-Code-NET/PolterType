//! macOS keyboard listener + emitter.
//!
//! ## Listener
//!
//! Built on `CGEventTapCreate(kCGSessionEventTap, …, listenOnly)`,
//! attached to the `CFRunLoop` of a dedicated thread. macOS requires
//! the calling app to be granted **Accessibility** in
//! System Settings → Privacy & Security → Accessibility. We surface
//! that requirement to the user via the tray onboarding banner; if
//! the tap fails to attach (typical first-launch state), `start()`
//! returns `InputError::Os` so the engine gracefully degrades.
//!
//! The tap subscribes to `KeyDown`, `KeyUp` **and** `FlagsChanged` —
//! the last is the only way macOS reports a modifier moving, and
//! without it `held_modifiers` only refreshed on ordinary keystrokes.
//!
//! ## Emitter
//!
//! `CGEventPost` with `CGEventKeyboardSetUnicodeString` — same
//! layout-independent contract as Windows' `KEYEVENTF_UNICODE`.
//!
//! > **Status:** validated end-to-end on macOS 15 (Intel): the tap
//! > receives events, corrections emit, and injected events are
//! > recognised via the user-data tag. The `FlagsChanged` subscription
//! > and `release_modifiers` (0.6.4) have not yet run on hardware.
//!
//! ## Why this is a directory
//!
//! `codes` holds the keyboard facts — the Apple → SC Set-1 table and
//! the rules that turn a `FlagsChanged` event into a press or a
//! release. It deliberately depends on nothing Apple-specific, so it
//! compiles under `cfg(test)` on every host and its tests run in CI on
//! Linux and Windows too. Everything that touches `core-graphics` is
//! macOS-only and can only be compiled by CI's `macos-latest` job.

pub(crate) mod codes;

#[cfg(target_os = "macos")]
mod consts;
#[cfg(target_os = "macos")]
mod emitter;
#[cfg(target_os = "macos")]
mod listener;

#[cfg(test)]
mod tests;

#[cfg(target_os = "macos")]
pub use emitter::MacosEmitter;
#[cfg(target_os = "macos")]
pub use listener::MacosListener;
