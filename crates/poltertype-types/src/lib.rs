//! Shared primitive types for poltertype.
//!
//! Intentionally minimal & OS-agnostic. Anything platform-specific
//! lives in `poltertype-input` / `poltertype-layout`.

#![forbid(unsafe_code)]

pub mod ai_plugin;
mod consts;
mod enums;
mod lam_alef;
pub mod logsafe;
mod types;

pub use consts::*;
pub use enums::*;
pub use lam_alef::{fold_lam_alef, lam_alef_pair};
pub use types::*;

#[cfg(test)]
mod tests;
pub use ai_plugin::AiPluginConfig;
