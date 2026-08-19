//! Shared types for the autostart backends.

/// The identity an autostart entry is written under.
///
/// A struct rather than parallel `&str` parameters, which would invite
/// a silent swap: an entry named `dev.opensource.poltertype` with
/// `Name=dev.opensource.poltertype` still "works", so nothing catches
/// it.
#[derive(Debug, Clone, Copy)]
pub struct App<'a> {
    /// Reverse-DNS identifier. The launchd label and plist stem on
    /// macOS, the `.desktop` file stem on Linux, the run-key value
    /// name on Windows. Must be stable across releases: it is how we
    /// find the entry we wrote last time.
    pub id: &'a str,
    /// Human-readable name, for the places an OS shows the entry to
    /// the user — today only the Linux `.desktop` `Name=`.
    pub name: &'a str,
    /// Icon-theme name, for the same places — today only the Linux
    /// `.desktop` `Icon=`. Deliberately **not** [`id`](Self::id): an
    /// icon theme is keyed on whatever the packages installed the mark
    /// under, which is `poltertype`, not the reverse-DNS form. Omitting
    /// `Icon=` is what puts a placeholder beside our name in the
    /// session's "Startup Applications" list.
    pub icon: &'a str,
}
