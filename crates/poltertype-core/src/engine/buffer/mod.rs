//! Word buffer: accumulate keystrokes until the user finishes a word.
//!
//! "Finishes" = the user produces a key whose **character** under the
//! current layout is a word boundary (whitespace, sentence
//! punctuation, brackets, …). Backspace also deletes the most recent
//! buffered key.
//!
//! Why we look at the *character*, not the raw scancode: the same
//! physical key produces wildly different things across layouts.
//! Scancode `0x33` is `,` (a boundary) under en-US but the letter
//! `б` (a word character) under uk-UA — and we want `б` to land in
//! the buffer so words like `будь` are detected correctly. Earlier
//! versions of this module classified by scancode alone and silently
//! split Cyrillic words at every `б` / `ю` / similar.
//!
//! ## Screen-sync model
//!
//! The engine deletes exactly the characters it believes sit left of
//! the caret. Any place where the buffer's idea and the screen's
//! reality can diverge therefore turns a correction into text
//! corruption ("half the word switched layouts", "the first letter
//! stayed behind"). Two mechanisms keep them in sync:
//!
//! * **Previous-word re-open.** After a word completes, its keys stay
//!   stashed together with the run of boundary keys typed after it.
//!   Backspacing over the boundary re-opens the previous word, so
//!   "type a word, delete a couple of chars, retype them" keeps the
//!   buffer byte-for-byte in step with the screen. This works even
//!   after our own correction replay, because the buffer stores
//!   layout-independent scancodes.
//!
//! * **Poisoning.** When the buffer *knows* it lost track (backspace
//!   into text it never saw, caret moved via arrows / Home / End /
//!   mouse click mid-word, idle gap mid-word, a shortcut fired
//!   mid-word), it marks the in-progress word tainted. A tainted
//!   completion must never be auto-corrected — correcting a word we
//!   only partially observed is how words get chopped in half. The
//!   taint clears at the next boundary: the following word is tracked
//!   from its first key and is trustworthy again.

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
