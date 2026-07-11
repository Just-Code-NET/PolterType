//! `UserCommand` — one `[[commands]]` entry from `config.toml`.

use super::*;
use serde::{Deserialize, Serialize};

/// A single user-defined smart command. Saved as a `[[commands]]`
/// entry in `config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserCommand {
    /// Stable identifier. Must be unique across the user's command
    /// list. Used in logs and as a stable handle for the UI; never
    /// shown to end users on its own (we render `name` instead).
    /// Must NOT collide with [`BUILTIN_PAUSE_TOGGLE_ID`] or
    /// [`BUILTIN_SWITCH_LAST_ID`].
    pub id: String,
    /// Display name shown in the Settings UI's Commands pane.
    /// Free-form; falls back to `id` if empty.
    #[serde(default)]
    pub name: String,
    /// The token the user types to fire this command. Matched
    /// exactly (case-sensitive) against the just-completed word
    /// at every boundary. Must not contain whitespace — the buffer
    /// resets at word boundaries, so a multi-token trigger could
    /// never match.
    ///
    /// Examples:
    ///
    /// * `anrl` — common acronym, expands to a phrase.
    /// * `:date:` — Espanso-style delimited trigger that won't
    ///   collide with normal words.
    /// * `;sig` — leading punctuation that's also rare in prose.
    ///
    /// Choose triggers that don't collide with words you actually
    /// type — `the` would expand on every English sentence.
    pub trigger: String,
    /// What to do when the trigger fires. Tagged-union TOML — the
    /// `type` key picks the variant.
    pub action: CommandAction,
    /// Optional list of foreground app basenames this command is
    /// active in. Empty = active everywhere. Match is case-
    /// insensitive against the focused process's exe basename, the
    /// same comparison [`crate::settings::ExceptionSettings`] uses.
    #[serde(default)]
    pub apps: Vec<String>,
}
