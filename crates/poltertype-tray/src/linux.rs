//! Keeping the GTK tray backend's chatter out of the user's stderr,
//! and catching its absence before it aborts the process.

use tracing::debug;

/// GLib log domain of the library `tray-icon` loads for the tray.
const APPINDICATOR_DOMAIN: &str = "libayatana-appindicator";

/// Sonames `libappindicator-sys` tries, in its own order. Kept in sync
/// with that crate's `LIB` initialiser — the last two only exist there
/// under its `backcompat` feature, and trying them here too costs
/// nothing and cannot report a false negative.
const APPINDICATOR_SONAMES: [&str; 4] = [
    "libayatana-appindicator3.so.1",
    "libappindicator3.so.1",
    "libayatana-appindicator3.so",
    "libappindicator3.so",
];

/// Why the tray cannot be built here, or `None` when it can.
///
/// `libappindicator-sys` `dlopen`s the tray library on first use and
/// **panics** when no soname resolves. Release builds are `panic =
/// "abort"`, so that arrives as a SIGABRT with a dlopen dump in the
/// user's system language and no hint of what to install
/// ([#31](https://github.com/Just-Code-NET/PolterType/issues/31)) —
/// `catch_unwind` cannot help. Asking first is the only way to say
/// something useful.
///
/// A resolved handle is deliberately leaked: the tray is about to load
/// the same object anyway, and `dlclose`ing a GTK-linked library only
/// to reopen it is the riskier half of the trade.
pub fn unavailable_reason() -> Option<String> {
    for soname in APPINDICATOR_SONAMES {
        // SAFETY: opening a library runs its initialisers, which is
        // exactly what the tray does moments later. No symbol is
        // called through this handle.
        if let Ok(lib) = unsafe { libloading::Library::new(soname) } {
            debug!(soname, "tray library present");
            std::mem::forget(lib);
            return None;
        }
    }
    Some(format!(
        "The system tray library is missing — PolterType puts its whole UI in the tray \
         and cannot start without it.\n\
         Install one of: {}\n\
         \n  \
         Arch / CachyOS / Manjaro:  sudo pacman -S libayatana-appindicator\n  \
         Debian / Ubuntu:           sudo apt install libayatana-appindicator3-1\n  \
         Fedora:                    sudo dnf install libayatana-appindicator-gtk3\n  \
         openSUSE:                  sudo zypper install libayatana-appindicator3-1",
        APPINDICATOR_SONAMES.join(", ")
    ))
}

/// Route `libayatana-appindicator`'s warnings into our own log.
///
/// Building the tray makes the library `g_warning()` a deprecation
/// notice straight to stderr, on every start, on any distro carrying a
/// recent enough build of it.
///
/// It is addressed to whoever links the library, which is not us:
/// `tray-icon` reaches it through `libappindicator-sys`, which
/// `dlopen`s the object by name. There is no feature to flip and no
/// newer release to move to — `tray-icon` 0.24, five versions ahead of
/// ours, still loads the same object. So neither the user nor we can
/// act on the message.
///
/// Hence redirect rather than silence: the handler hands the text to
/// `tracing` at debug level, where it stays available the day the
/// library actually goes away, without landing in the journal of
/// everyone running a tray app. This domain only; every other GLib
/// domain keeps GLib's default handler.
///
/// Call once, before the `TrayIcon` is built.
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
