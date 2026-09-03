//! macOS: register the sending bundle, once, before the first toast.

use std::sync::OnceLock;

use tracing::{info, warn};

/// Whether a notification may be sent — and, the first time, the
/// registration that makes it true.
///
/// `mac-notification-sys` needs a sender bundle id. When nobody set
/// one, its `ensure_application_set` asks LaunchServices for an app
/// literally named `use_default` (lib.rs:116 in 0.6.12) — and macOS
/// answers an unresolvable name with a modal **"Where is
/// use_default?" application chooser** over whatever the user was
/// doing. Observed live; and because the updater's error toasts bypass
/// the `show_notifications` gate, the dialog can appear even with
/// notifications switched off in the config.
///
/// `false` means there is no bundle to speak as — a bare binary run
/// from `cargo run` — and the caller must skip its toast: a missing
/// notification is cheaper than that dialog.
pub fn notification_sender_ready() -> bool {
    static READY: OnceLock<bool> = OnceLock::new();
    *READY.get_or_init(|| {
        let Some(bundle_id) = main_bundle_identifier() else {
            info!("not running from an .app bundle; system notifications stay off");
            return false;
        };
        match notify_rust::set_application(&bundle_id) {
            Ok(()) => {
                info!(bundle = %bundle_id, "registered as notification sender");
                true
            }
            Err(e) => {
                warn!(?e, bundle = %bundle_id, "could not register as notification sender");
                false
            }
        }
    })
}

/// Our own `CFBundleIdentifier`, or `None` outside an `.app` bundle.
fn main_bundle_identifier() -> Option<String> {
    use core_foundation::string::CFString;
    let dict = core_foundation::bundle::CFBundle::main_bundle().info_dictionary();
    let key = CFString::from_static_string("CFBundleIdentifier");
    dict.find(&key)
        .and_then(|v| v.downcast::<CFString>())
        .map(|s| s.to_string())
}
