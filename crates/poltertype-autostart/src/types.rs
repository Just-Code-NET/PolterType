//! Shared types for the autostart backends.

/// The identity an autostart entry is written under.
///
/// Two `&str` parameters side by side would invite a silent swap —
/// both are strings, and an entry named `dev.opensource.poltertype`
/// with `Name=dev.opensource.poltertype` still "works", so nothing
/// would catch it. Naming them costs five lines.
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
}
