//! What a matched command does.

use super::shell::ShellCommand;

use poltertype_types::LayoutId;
use serde::{Deserialize, Serialize};

/// Tagged enum of available actions. The TOML representation is
/// `type = "<snake_case>"` plus the variant's payload fields:
///
/// ```toml
/// [[commands]]
/// id      = "anrl"
/// trigger = "anrl"
/// action  = { type = "type_text", text = "Anatomical Reference List" }
///
/// [[commands]]
/// id      = "to-english"
/// trigger = "((en))"
/// action  = { type = "switch_layout", layout = "en-US" }
/// ```
///
/// Adding a variant is forward-compatible: an old binary fails to parse
/// the unknown `type`, logs one warning, and keeps the rest of the
/// config.
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
    /// Run a program. **Off unless `[commands].allow_run_shell` is
    /// true**, executed without a shell, and never given anything the
    /// user typed as an argument. `crate::commands::shell` carries
    /// the threat model; read it before changing any of that.
    RunShell(ShellCommand),
}
