//! Word buffer: accumulate keystrokes until a key whose **character**
//! under the current layout is a boundary. The character, not the raw
//! scancode: `0x33` is `,` under en-US but the letter `б` under uk-UA,
//! and classifying by scancode silently split Cyrillic words at every
//! `б` / `ю`.
//!
//! **Screen-sync model.** The engine deletes exactly the characters it
//! believes sit left of the caret, so any divergence between the
//! buffer's idea and the screen turns a correction into corruption.
//! Two mechanisms keep them together:
//!
//! * **Previous-word re-open.** A completed word's keys stay stashed
//!   with the boundary keys typed after it, so backspacing over the
//!   boundary re-opens it. Survives our own replay too, because the
//!   stash is layout-independent scancodes.
//! * **Poisoning.** When the buffer *knows* it lost track — backspace
//!   into text it never saw, caret moved mid-word, idle gap, a shortcut
//!   — it taints the in-progress word, and a tainted completion is
//!   never auto-corrected. The taint clears at the next boundary.

mod classify;
mod consts;
mod enums;
mod word_buffer;

pub(crate) use classify::*;
pub(crate) use consts::*;
pub use enums::*;
pub use word_buffer::*;

#[cfg(test)]
mod tests;
