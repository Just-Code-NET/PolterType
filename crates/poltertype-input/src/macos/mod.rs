//! macOS keyboard listener + emitter.
//!
//! The listener is a `CGEventTapCreate(kCGSessionEventTap, …,
//! listenOnly)` attached to the `CFRunLoop` of a dedicated thread, and
//! needs **Accessibility** granted in System Settings. A tap that fails
//! to attach — the typical first-launch state — makes `start()` return
//! `InputError::Os`, and the tray surfaces the onboarding banner.
//!
//! It subscribes to `KeyDown`, `KeyUp` **and** `FlagsChanged`, the last
//! being the only way macOS reports a modifier moving.
//!
//! The emitter is `CGEventPost` with
//! `CGEventKeyboardSetUnicodeString` — the same layout-independent
//! contract as Windows' `KEYEVENTF_UNICODE`.
//!
//! A directory rather than one file because [`codes`] holds the
//! keyboard facts and depends on nothing Apple-specific, so it compiles
//! under `cfg(test)` on every host and its tests run in CI on Linux and
//! Windows too. Everything touching `core-graphics` can only be
//! compiled by CI's `macos-latest` job.

pub(crate) mod codes;

#[cfg(target_os = "macos")]
mod consts;
#[cfg(target_os = "macos")]
mod emitter;
#[cfg(target_os = "macos")]
mod gate;
#[cfg(target_os = "macos")]
mod listener;

#[cfg(test)]
mod tests;

#[cfg(target_os = "macos")]
pub use emitter::MacosEmitter;
#[cfg(target_os = "macos")]
pub use gate::MacosGate;
#[cfg(target_os = "macos")]
pub use listener::MacosListener;
