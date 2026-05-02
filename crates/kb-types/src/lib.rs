//! Shared primitive types for kb-switcher.
//!
//! Intentionally minimal & OS-agnostic. Anything platform-specific
//! lives in `kb-input` / `kb-layout`.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

// ─── Layout identifier ───────────────────────────────────────────────

/// BCP-47-ish identifier for a keyboard layout (`en-US`, `uk-UA`,
/// `kk-Cyrl-KZ`, `hy-AM`, …). Stored as an opaque string so we never
/// have to enumerate every possible layout in Rust code; the value
/// comes from the OS or from data files in `data/layout-mappings/`.
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

impl From<&str> for LayoutId {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

// ─── Key events ──────────────────────────────────────────────────────

/// A raw keyboard event captured by the per-OS listener.
///
/// Deliberately keeps OS-specific codes (`vk`, `scancode`) so that the
/// engine can translate via layout-mapping tables without losing
/// fidelity. Higher-level translation (vk → char) happens in
/// `kb-detect` / `kb-core`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    /// OS-specific virtual key code.
    pub vk: u32,
    /// OS-specific hardware scancode.
    pub scancode: u32,
    pub direction: KeyDirection,
    pub modifiers: Modifiers,
    /// True if the event was synthesised (e.g. by `SendInput` from us
    /// or another app). Hooks must skip these to avoid feedback loops.
    pub injected: bool,
    /// Best-effort monotonic timestamp in ms.
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyDirection {
    Press,
    Release,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub meta: bool,
}

impl Modifiers {
    pub const NONE: Self = Self {
        shift: false,
        control: false,
        alt: false,
        meta: false,
    };

    pub fn is_empty(&self) -> bool {
        !(self.shift || self.control || self.alt || self.meta)
    }
}
