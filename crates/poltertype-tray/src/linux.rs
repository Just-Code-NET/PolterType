//! Keeping the GTK tray backend's chatter out of the user's stderr.

use tracing::debug;

/// GLib log domain of the library `tray-icon` loads for the tray.
const APPINDICATOR_DOMAIN: &str = "libayatana-appindicator";

/// Route `libayatana-appindicator`'s warnings into our own log.
///
/// Building the tray makes the library `g_warning()` a deprecation
/// notice — *"libayatana-appindicator is deprecated. Please use
/// libayatana-appindicator-glib in newly written code."* — straight to
/// stderr, on every start, on any distro carrying a recent enough
/// build of it.
///
/// It is addressed to whoever links the library, which is not us:
/// `tray-icon` reaches it through `libappindicator-sys`, which
/// `dlopen`s `libayatana-appindicator3.so.1` by name. There is no
/// feature to flip (`backcompat` only adds unversioned-`.so`
/// fallbacks) and no newer release to move to — `tray-icon` 0.24, five
/// versions ahead of ours, still loads the same object. So the
/// user cannot act on the message and neither can we.
///
/// Hence redirect rather than silence: the handler hands the text to
/// `tracing` at debug level, where it stays available to us the day
/// the library actually goes away, without landing in the journal of
/// everyone running a tray app. Warnings from this one domain only —
/// every other GLib domain keeps GLib's default handler.
///
/// Call once, before the `TrayIcon` is built; installing a handler is
/// cheap and takes effect for the life of the process.
pub fn quiet_gtk_tray_logs() {
    glib::log_set_handler(
        Some(APPINDICATOR_DOMAIN),
        glib::LogLevels::LEVEL_WARNING,
        // Not fatal, and no recursion guard needed: the closure only
        // reaches `tracing`, never GLib.
        false,
        false,
        |_domain, _level, message| debug!(message, "libayatana-appindicator"),
    );
}
