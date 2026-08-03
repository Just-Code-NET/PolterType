//! Per-OS quirks of the desktop app shell.
//!
//! Three unrelated things live here for one reason: each is a place
//! where an operating system disagrees with the others about
//! something the binary would otherwise have to know, and
//! `poltertype-app` holds no `#[cfg(target_os)]` (see
//! `CONTRIBUTING.md`). They are not an abstraction — there is nothing
//! common between a lock file, a Dock policy and a keycap glyph. They
//! are the app shell's platform surface, collected so the binary can
//! stay platform-blind.
//!
//! | What | Diverges because |
//! |---|---|
//! | [`instance_lock_id`] | `single-instance` means a different thing by "id" per OS |
//! | [`keep_out_of_dock`] | only macOS has a Dock, and `tao` overrides `LSUIElement` |
//! | [`key_glyph`] | macOS prints glyphs on the keys, the others print words |

#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod instance;
mod keys;
mod process;

#[cfg(test)]
mod tests;

pub use instance::{InstanceLock, acquire as acquire_instance_lock};
pub use keys::{key_glyph, key_name_with_glyph};
pub use process::request_stop;

/// Keep a tray-only app out of the Dock and the app switcher.
///
/// `LSUIElement` in the bundle's `Info.plist` is not enough on its
/// own: `tao` applies `ActivationPolicy::Regular` at startup and
/// overrides it, so a tray app shows a Dock icon anyway. Must be
/// called before the event loop runs — afterwards the policy is fixed
/// for the process.
///
/// Nothing to do anywhere else: no other platform we ship has a Dock,
/// and the tray icon is the whole UI surface on all of them.
pub fn keep_out_of_dock<T>(event_loop: &mut tao::event_loop::EventLoop<T>) {
    #[cfg(target_os = "macos")]
    {
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        event_loop.set_activation_policy(ActivationPolicy::Accessory);
    }
    // No Dock to stay out of anywhere else.
    #[cfg(not(target_os = "macos"))]
    let _ = event_loop;
}
