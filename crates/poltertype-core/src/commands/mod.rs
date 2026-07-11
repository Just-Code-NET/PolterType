//! Smart commands — text-trigger expansions and shortcuts.
//!
//! Inspired by classic text expanders (TextExpander, Espanso,
//! AutoHotkey hotstrings): the user types a short token like
//! `anrl ` (acronym + space), the engine recognises it on the word
//! boundary, deletes the token + boundary, and runs an action —
//! typically expanding to a longer phrase.
//!
//! ## Why text triggers, not hotkeys
//!
//! `[hotkeys]` already gives users two global key combinations
//! (pause, switch-last). Adding more global hotkeys is a separate
//! UX choice — they collide with system-wide bindings, they're
//! invisible (a typed trigger is right there in your text), and
//! the OS imposes a hard limit on how many you can register. Text
//! triggers don't have any of those constraints — they live
//! entirely inside poltertype's word-boundary pipeline (the same
//! pipeline that already does layout-aware corrections), so users
//! can have hundreds of them with no performance / UX cost.
//!
//! ## Activation
//!
//! The engine consults the configured triggers on every word
//! boundary, BEFORE the structural-boundary / disabled-app /
//! identifier filters. Order is significant:
//!
//!   1. User types `anrl<space>`.
//!   2. Word boundary fires.
//!   3. Trigger lookup: `anrl` matches → dispatch action,
//!      backspace the typed token + boundary, re-emit any text
//!      the action wants to leave behind, return.
//!   4. (Otherwise) normal layout-correction pipeline runs.
//!
//! Putting trigger lookup BEFORE the identifier / app filters means
//! a snippet like `=>` works inside an IDE — those filters would
//! otherwise veto auto-switching, but text expansion is what the
//! user actively asked for, so the filters don't apply.
//!
//! ## v1 action surface
//!
//! Deliberately small. Each variant maps to one OS primitive we
//! already know how to do safely:
//!
//! * [`CommandAction::TypeText`] → `KeyEmitter::send_text`
//! * [`CommandAction::SwitchLayout`] → `LayoutSwitcher::switch_to`
//! * [`CommandAction::OpenPath`] → `opener::open`
//!
//! The most common use case is `TypeText` (snippet expansion); the
//! other two are power-user shortcuts that happen to fit the same
//! "type a magic word, something happens" model.
//!
//! What's intentionally **not** here in v1:
//!
//! * `RunShell { argv }` — full command execution. The blast radius
//!   (a malicious `[[commands]]` entry in a stolen config could
//!   mass-exfiltrate) makes this a separate security review.
//! * Multi-token triggers (`best regards` → `…`). The buffer is
//!   reset at every word boundary; matching across boundaries
//!   needs a sliding window we don't have today.
//! * Case-insensitive / case-preserving expansion. v1 matches
//!   exactly — users pick triggers that don't collide with prose.

mod consts;
mod enums;
mod matching;
mod types;

pub use consts::*;
pub use enums::*;
pub use matching::*;
pub use types::*;

#[cfg(test)]
mod tests;
