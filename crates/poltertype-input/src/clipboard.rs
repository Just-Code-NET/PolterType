//! Reading and writing the system clipboard from a process with no
//! window.
//!
//! Only one feature needs this — converting a *selection* rather than
//! the last word (issue #32) — and it needs it under a hard
//! constraint: PolterType is tray-only and must never take keyboard
//! focus. Focus is the thing it exists not to disturb.
//!
//! On Wayland that rules out the ordinary `wl_data_device`, which
//! serves the focused surface. The usable route is the data-control
//! protocol — `ext_data_control_manager_v1`, or its `zwlr_` predecessor
//! — which exists precisely for clipboard managers and other windowless
//! clients. `wl-clipboard-rs` speaks only those two and has no
//! focus-stealing fallback, which is why a compositor without either
//! comes back as *unavailable* rather than as a stolen focus.
//!
//! Measured across the desktop matrix (2026-08-28): KDE Plasma
//! advertises `ext_data_control` only, sway / labwc / Budgie advertise
//! both, and **GNOME and Cinnamon's Wayland sessions advertise
//! neither**. On those two the feature cannot work the way this app
//! requires, and says so.

use crate::InputError;

pub use crate::traits::Clipboard;

/// Why the clipboard is unavailable here, in words a Setup pane can
/// show without the reader knowing what a Wayland protocol is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardGap {
    /// The compositor offers no data-control protocol, so the only way
    /// to read the clipboard would be to take focus.
    NoWindowlessAccess,
    /// The platform has a clipboard but this build could not open it.
    Unavailable(String),
    /// The clipboard is fine; this build cannot press the copy chord
    /// that would fill it. macOS, until its emitter grows `send_chord`.
    NoCopyChord,
}

impl std::fmt::Display for ClipboardGap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoWindowlessAccess => write!(
                f,
                "this desktop does not let a background app read the clipboard \
                 without taking keyboard focus, which PolterType will not do"
            ),
            Self::Unavailable(why) => write!(f, "the clipboard could not be opened: {why}"),
            Self::NoCopyChord => write!(
                f,
                "PolterType cannot yet send the copy shortcut on this platform, \
                 so it has no way to read what you have selected"
            ),
        }
    }
}

/// The system clipboard as `arboard` sees it.
///
/// **Reads** open a fresh handle per call rather than one held open: on
/// Wayland the handle owns a connection and, once we have written, the
/// selection itself, and a long-lived one would keep serving stale data
/// after the user copied something else. Opening costs a socket round
/// trip, and this runs once per force-switch on a word that is not
/// there.
///
/// **Writes** cannot do that, and doing it is why selection conversion
/// never worked on Linux. Both X11 and Wayland serve the clipboard from
/// the process that owns it, so a write is only as durable as the
/// handle that made it — `arboard` tears its serving window down with
/// the last one, and the text is gone before anything can paste it.
/// Measured 2026-09-02 on a nested X server with no clipboard manager,
/// on XWayland and on Hyprland's data-control protocol: a marker
/// written through a dropped handle reads back as an *empty* clipboard
/// on all three, and survives on all three when the handle is kept.
///
/// That destroyed the converted text before the paste chord went out,
/// and took the user's own clipboard with it on the way back — the
/// restore was written through a temporary handle too (issue #51).
/// Windows is unaffected: its clipboard is owned by the OS, not served
/// by the writer, which is why the same code was confirmed working
/// there (issue #32).
///
/// So the writing handle stays. This is a tray app; it outlives the
/// paste by design, which is exactly the lifetime the protocols ask
/// for.
struct SystemClipboard {
    writer: parking_lot::Mutex<Option<arboard::Clipboard>>,
}

impl SystemClipboard {
    fn new() -> Self {
        Self {
            writer: parking_lot::Mutex::new(None),
        }
    }
}

impl Clipboard for SystemClipboard {
    fn text(&self) -> Result<Option<String>, InputError> {
        let mut cb = arboard::Clipboard::new().map_err(map_err)?;
        match cb.get_text() {
            Ok(t) => Ok(Some(t)),
            // Not an error: the clipboard holding an image, or nothing
            // at all, is a normal state and the caller must be able to
            // tell it from a failure to look.
            Err(arboard::Error::ContentNotAvailable) => Ok(None),
            Err(e) => Err(map_err(e)),
        }
    }

    fn set_text(&self, text: &str) -> Result<(), InputError> {
        let mut writer = self.writer.lock();
        let cb = match writer.as_mut() {
            Some(cb) => cb,
            None => writer.insert(arboard::Clipboard::new().map_err(map_err)?),
        };
        cb.set_text(text.to_owned()).map_err(map_err)
    }
}

fn map_err(e: arboard::Error) -> InputError {
    InputError::Os(format!("clipboard: {e}"))
}

/// Open the clipboard, or say why not.
///
/// The probe is an actual open, not a guess from the environment: the
/// question "can this build reach this session's clipboard" has one
/// honest answer and it is the one the library gives.
pub fn clipboard() -> Result<Box<dyn Clipboard>, ClipboardGap> {
    match arboard::Clipboard::new() {
        Ok(_) => Ok(Box::new(SystemClipboard::new())),
        Err(e) => {
            let why = e.to_string();
            // `wl-clipboard-rs` speaks only the data-control protocols
            // and has no focus-stealing fallback, so a compositor
            // without either fails here — GNOME and Cinnamon's Wayland
            // sessions, measured. Naming that case separately is what
            // lets the Setup pane explain it instead of showing a
            // library string.
            if cfg!(target_os = "linux")
                && std::env::var_os("WAYLAND_DISPLAY").is_some()
                && (why.contains("data_control") || why.contains("data control"))
            {
                Err(ClipboardGap::NoWindowlessAccess)
            } else {
                Err(ClipboardGap::Unavailable(why))
            }
        }
    }
}

/// Can this build convert a *selection* on this machine at all?
///
/// Two things have to be true and they fail differently. The clipboard
/// has to be reachable without taking focus, which is a property of the
/// session and is probed. And the emitter has to be able to hold
/// modifiers around a key, to press the copy chord in the first place —
/// which every desktop platform's emitter now can: macOS was the last
/// holdout, gated here until its `send_chord` existed, and posts the
/// chord as `Cmd`-flagged key events since it does.
///
/// Kept beside the clipboard rather than in the Settings window so
/// there is one answer, and the window and the engine cannot disagree
/// about it.
pub fn selection_support() -> Result<(), ClipboardGap> {
    clipboard().map(|_| ())
}

#[cfg(test)]
mod tests;
