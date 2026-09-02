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

/// The system clipboard, as much of it as this app has any business
/// touching: text in, text out.
pub trait Clipboard: Send + Sync {
    /// The clipboard's current text, or `None` when it holds something
    /// that is not text (an image, a file list). `None` is not an
    /// error: it is the answer that stops a caller replacing an image
    /// with a string.
    fn text(&self) -> Result<Option<String>, InputError>;

    /// Replace the clipboard's contents with `text`.
    ///
    /// On Wayland this makes *us* the owner of the selection, so the
    /// data lives as long as this process does — fine for a tray app
    /// that outlives the interaction, and the reason restoring what was
    /// there is possible at all.
    fn set_text(&self, text: &str) -> Result<(), InputError>;
}

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
mod tests {
    use super::*;

    /// Reports whether *this* session lets a windowless process reach
    /// the clipboard, and round-trips a marker through it if so.
    ///
    /// `cargo test -p poltertype-input -- --ignored --nocapture clipboard_of_this_session`
    ///
    /// Ignored because the answer is a property of the machine, not of
    /// the code — and it is the answer the desktop matrix collects, one
    /// session at a time. Asserting anything here would fail every run
    /// on GNOME, where the honest result is "unavailable".
    /// A write has to outlive the call that made it.
    ///
    /// The one property selection conversion rests on, and the one it
    /// was missing: the converted text is staged, a paste chord goes
    /// out, and the focused application reads the clipboard some
    /// milliseconds later — from whichever process still owns it. A
    /// write that dies with its handle reads back as an empty
    /// clipboard, which is what issue #51 was, on every Linux backend.
    ///
    /// `cargo test -p poltertype-input -- --ignored --nocapture a_write_outlives_the_call`
    ///
    /// Ignored because it needs a real session; there is no clipboard
    /// on a CI runner, and a skip that passes would say nothing.
    #[test]
    #[ignore = "needs a real desktop session's clipboard"]
    fn a_write_outlives_the_call() {
        let Ok(cb) = clipboard() else {
            println!("no windowless clipboard in this session — nothing measured");
            return;
        };
        let before = cb.text().ok().flatten();
        let marker = format!("poltertype-durability-{}", std::process::id());
        let staged = cb.set_text(&marker);
        assert!(staged.is_ok(), "could not stage the marker: {staged:?}");

        // Read through a *fresh* handle, and after the pause a paste
        // really takes: reading back through the one that wrote would
        // prove nothing, since that handle is the thing under test.
        std::thread::sleep(std::time::Duration::from_millis(400));
        let read = cb.text();

        // Before the assertion, so a failure does not also walk off
        // with the session's clipboard.
        if let Some(prev) = before {
            let _ = cb.set_text(&prev);
        }
        assert_eq!(
            read.ok().flatten().as_deref(),
            Some(marker.as_str()),
            "the staged text must still be there when the paste asks for it"
        );
    }

    #[test]
    #[ignore = "reports this session's real clipboard access; nothing to assert"]
    fn clipboard_of_this_session() {
        let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "?".into());
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "?".into());
        match clipboard() {
            Err(gap) => {
                println!("CLIPBOARD session={session} desktop={desktop} available=no gap={gap:?}");
                println!("  reads as: {gap}");
            }
            Ok(cb) => {
                let before = cb.text();
                let marker = format!("poltertype-probe-{}", std::process::id());
                let wrote = cb.set_text(&marker);
                std::thread::sleep(std::time::Duration::from_millis(300));
                let read = cb.text();
                let ok = matches!(&read, Ok(Some(t)) if *t == marker);
                println!(
                    "CLIPBOARD session={session} desktop={desktop} available=yes \
                     roundtrip={ok} wrote={:?} read_ok={}",
                    wrote.is_ok(),
                    read.is_ok()
                );
                // Put back whatever was there, so a sweep does not walk
                // off with the session's clipboard.
                if let Ok(Some(prev)) = before {
                    let _ = cb.set_text(&prev);
                }
            }
        }
    }
}
