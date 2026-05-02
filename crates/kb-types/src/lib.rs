//! Shared primitive types for kb-switcher.
//!
//! Phase 1 placeholder. Real types land in Phase 2/3.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

/// BCP-47-style identifier for a keyboard layout, e.g. `en-US`, `uk-UA`,
/// `kk-Cyrl-KZ`. Stored as an opaque string so we can support arbitrary
/// (including unusual) tags without enumerating them in Rust code.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LayoutId(pub String);

impl LayoutId {
    pub fn new(tag: impl Into<String>) -> Self {
        Self(tag.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for LayoutId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
