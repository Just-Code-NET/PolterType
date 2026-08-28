//! Per-OS quirks of the desktop app shell.
//!
//! Each is a place where one OS disagrees with the others about
//! something the binary would otherwise have to know, and
//! `poltertype-app` holds no `#[cfg(target_os)]` (see
//! `CONTRIBUTING.md`). Not an abstraction — there is nothing common
//! between a lock file, a Dock policy and a keycap glyph.
//!
//! | What | Diverges because |
//! |---|---|
//! | [`instance_lock_id`] | `single-instance` means a different thing by "id" per OS |
//! | [`keep_out_of_dock`] | only macOS has a Dock, and `tao` overrides `LSUIElement` |
//! | [`key_glyph`] | macOS prints glyphs on the keys, the others print words |
//! | [`update_permission_note`] | only macOS makes an update cost the user their permissions |
//! | [`configure_child`] | only Windows hands a console program a window |
//! | [`request_stop`] | only Unix has a signal to ask a process to leave |
//! | [`window_platform_specific`] | only Linux ties a window to an app id, and the field exists only there |
//! | [`install_desktop_entry`] | only Linux keeps an app's name and icon in a third file |
//! | [`ui_font_family`] | each OS names its UI font differently, and only Linux can be asked |

#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod desktop;
mod font;
mod instance;
mod keys;
mod process;

#[cfg(test)]
mod tests;

pub use desktop::{DESKTOP_ID, install_desktop_entry, window_platform_specific};
pub use font::ui_font_family;
pub use instance::{InstanceLock, acquire as acquire_instance_lock};
pub use keys::{key_glyph, key_name_with_glyph, update_permission_note};
pub use process::{configure_child, request_stop};

/// Keep a tray-only app out of the Dock and the app switcher.
///
/// `LSUIElement` in the bundle's `Info.plist` is not enough on its own:
/// `tao` applies `ActivationPolicy::Regular` at startup and overrides
/// it. Must be called before the event loop runs — afterwards the
/// policy is fixed for the process.
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
