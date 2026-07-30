//! Shared primitive types for poltertype.
//!
//! Intentionally minimal & OS-agnostic. Anything platform-specific
//! lives in `poltertype-input` / `poltertype-layout`.

#![forbid(unsafe_code)]

mod consts;
mod enums;
pub mod logsafe;
mod types;

pub use consts::*;
pub use enums::*;
pub use types::*;

#[cfg(test)]
mod tests;
