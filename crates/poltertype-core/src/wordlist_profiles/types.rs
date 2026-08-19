//! Profile schema: `[wordlists]` settings and one profile entry.

use serde::{Deserialize, Serialize};

/// Top-level wordlist settings — sits under `[wordlists]` in
/// `config.toml`. The default is empty: no profiles, global overlay
/// only.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct WordlistSettings {
    /// Profile id used when no profile's `apps` list matches the focused
    /// app. Empty (the default) means the global overlay files only. A
    /// non-empty value must name an entry in `profiles`; an unknown id
    /// warns at engine start and falls back to global-only.
    pub default_profile: String,
    /// Configured profiles. Order matters only for the "first match
    /// wins" rule when two profiles list the same app.
    pub profiles: Vec<WordlistProfile>,
}

/// A single profile. The id picks the on-disk directory
/// (`<config-dir>/poltertype/wordlists/profiles/<id>/`); the
/// `apps` list picks when this profile is active.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WordlistProfile {
    /// Stable identifier. Must be unique across `profiles`.
    /// Recommended shape: kebab-case ASCII (`code`, `writing`,
    /// `gaming`). The id is also the directory name on disk, so
    /// avoid characters the OS doesn't like in path segments
    /// (`/`, `\\`, `:`, `*`, `?`, `"`, `<`, `>`, `|` on Windows).
    pub id: String,
    /// Free-form display name shown in the Settings UI, falling back to
    /// `id` if blank. Never matched against — that is `apps` only.
    #[serde(default)]
    pub name: String,
    /// Foreground apps this profile activates for. Each entry is
    /// matched case-insensitively against the focused process's
    /// exe basename — same comparison
    /// [`crate::settings::ExceptionSettings`] uses for
    /// `disabled_apps`, so users only learn one matching rule.
    /// Empty list = profile is never auto-activated (the user can
    /// still pick it manually as `default_profile`).
    #[serde(default)]
    pub apps: Vec<String>,
}
