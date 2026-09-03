//! Which font family the app's own windows are drawn in.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
mod other;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
use linux as imp;
#[cfg(target_os = "macos")]
use macos as imp;
#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
use other as imp;
#[cfg(target_os = "windows")]
use windows as imp;

/// A font family this machine really has, for the Settings window and
/// the suggestion tooltip — both draw through `cosmic-text`, whose
/// defaults resolve to a name ("Fira Sans") most machines do not have
/// and can fall through to a face with no text glyphs at all. See
/// docs/DECISIONS.md, 2026-08-27.
///
/// `None` means "no better idea than the default" — the caller keeps
/// whatever it was going to use.
pub use imp::ui_font_family;
