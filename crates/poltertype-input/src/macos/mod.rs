//! macOS keyboard listener + emitter.
//!
//! The listener is a `CGEventTapCreate(kCGSessionEventTap, …)` attached
//! to the `CFRunLoop` of a dedicated thread — listen-only unless the key
//! gate is switched on — and needs **Accessibility** granted in System
//! Settings. A tap that fails to attach, the typical first-launch state,
//! makes `start()` return `InputError::Os` and the tray surfaces the
//! onboarding banner.
//!
//! It subscribes to `KeyDown`, `KeyUp` **and** `FlagsChanged`, the last
//! being the only way macOS reports a modifier moving.
//!
//! The emitter is `CGEventPost` with
//! `CGEventKeyboardSetUnicodeString` — the same layout-independent
//! contract as Windows' `KEYEVENTF_UNICODE`.
//!
//! [`codes`] holds the keyboard facts and depends on nothing
//! Apple-specific, so it compiles under `cfg(test)` on every host;
//! everything touching `core-graphics` only by CI's `macos-latest` job.

pub(crate) mod codes;

#[cfg(target_os = "macos")]
mod consts;
#[cfg(target_os = "macos")]
mod emitter;
#[cfg(target_os = "macos")]
pub(crate) mod factory;
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
