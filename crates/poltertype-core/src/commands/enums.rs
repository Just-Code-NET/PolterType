//! What a matched command does.

use poltertype_types::LayoutId;
use serde::{Deserialize, Serialize};

/// Tagged enum of available actions. The TOML representation uses
/// `type = "<snake_case>"` plus the variant's payload fields, e.g.
///
/// ```toml
/// [[commands]]
/// id      = "anrl"
/// name    = "Anatomical reference list"
/// trigger = "anrl"
/// action  = { type = "type_text", text = "Anatomical Reference List" }
///
/// [[commands]]
/// id      = "to-english"
/// trigger = "((en))"
/// action  = { type = "switch_layout", layout = "en-US" }
///
/// [[commands]]
/// id      = "open-config"
/// trigger = ";cfg"
/// action  = { type = "open_path", path = "C:/Users/me/AppData/Roaming/poltertype/config.toml" }
/// ```
///
/// Adding a new variant is forward-compat: an old binary will fail
/// to parse the unknown `type` and the loader will keep the rest of
/// the config (one warning logged per skipped entry).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CommandAction {
    /// Type a literal string at the cursor. The engine first
    /// backspaces the trigger + the boundary character the user
    /// just typed, then emits this text, then re-emits the
    /// boundary character. So typing `anrl<space>` with text
    /// `Anatomical Reference List` produces
    /// `Anatomical Reference List<space>` — the space the user
    /// typed survives the expansion.
    TypeText { text: String },
    /// Switch the OS keyboard layout to the given id. Same
    /// pre-flight (`list_active`) as the corrector uses, so an
    /// unreachable layout is rejected loudly rather than silently.
    /// The trigger + boundary are deleted; nothing is re-emitted.
    SwitchLayout { layout: LayoutId },
    /// Open a path or URL via the user's default handler
    /// (`opener` crate). Files use the OS's MIME / extension
    /// mapping; URLs get the default browser. Trigger + boundary
    /// are deleted; nothing is re-emitted.
    OpenPath { path: String },
}
