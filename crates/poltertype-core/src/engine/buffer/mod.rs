//! Word buffer: accumulate keystrokes until the user finishes a word —
//! that is, produces a key whose **character** under the current layout
//! is a boundary. Backspace deletes the most recent buffered key.
//!
//! The character, not the raw scancode: `0x33` is `,` under en-US but
//! the letter `б` under uk-UA, and `б` has to land in the buffer for
//! `будь` to be detected. Classifying by scancode silently split
//! Cyrillic words at every `б` / `ю`.
//!
//! **Screen-sync model.** The engine deletes exactly the characters it
//! believes sit left of the caret, so anywhere the buffer's idea and
//! the screen can diverge turns a correction into corruption. Two
//! mechanisms keep them together:
//!
//! * **Previous-word re-open.** A completed word's keys stay stashed
//!   with the boundary keys typed after it, so backspacing over the
//!   boundary re-opens it and "type, delete a couple of chars, retype"
//!   stays byte-for-byte in step. Works after our own replay too,
//!   because the buffer stores layout-independent scancodes.
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
