//! Smart commands — text-trigger expansions and shortcuts: the user
//! types a token like `anrl `, the engine recognises it on the word
//! boundary and runs an action.
//!
//! Trigger lookup runs *before* the structural-boundary, disabled-app
//! and identifier filters, which is what makes a snippet work inside
//! an IDE. See `docs/ARCHITECTURE.md` § Smart commands for why text
//! triggers rather than hotkeys, and [`shell`] for the one action with
//! a threat model.

mod consts;
mod enums;
mod matching;
mod phrase;
mod shell;
mod types;

pub use consts::*;
pub use enums::*;
pub use matching::*;
pub use phrase::*;
pub use shell::*;
pub use types::*;

#[cfg(test)]
mod tests;
