//! The shared data structs: ids, key events, detection I/O.

use crate::*;
use serde::{Deserialize, Serialize};

/// BCP-47-ish identifier for a keyboard layout (`en-US`, `uk-UA`,
/// `kk-Cyrl-KZ`, `hy-AM`, …). Stored as an opaque string so we never
/// have to enumerate every possible layout in Rust code.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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

/// One keyboard **as the OS itself describes it**, produced by the
/// platform layout backend and consumed by the layout DB.
///
/// [`LayoutId`] names a *language*, which is all the OS layout APIs
/// agree on — but a language is not a keyboard. Bulgarian alone ships
/// three genuinely different Windows keyboards under the single id
/// `bg-BG`, and the bundled mapping can only describe one of them.
/// Asking the OS what its keys actually produce is the only way to
/// know which one is installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsKeymap {
    /// The language id this keyboard reports as — the same namespace
    /// `LayoutSwitcher::current()` and `list_active()` speak in.
    pub id: LayoutId,
    /// Opaque per-OS name for the *variant*: which of that language's
    /// keyboards this is. A Windows KLID (`"00030402"`), free-form
    /// elsewhere. Logged for diagnosis, never matched on.
    pub variant: String,
    /// `(scancode, unshifted, shifted)` for every key that produced a
    /// printable character. This is the **complete** character table
    /// for the keyboard: a scancode missing here produces nothing,
    /// which is why the layout DB replaces rather than merges.
    pub keys: Vec<(u32, char, Option<char>)>,
}

/// A raw keyboard event captured by the per-OS listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    /// OS-specific virtual-key code.
    pub vk: u32,
    /// Win SC Set-1 scancode (or normalized equivalent on macOS/Linux).
    pub scancode: u32,
    pub direction: KeyDirection,
    pub modifiers: Modifiers,
    /// True if the event was synthesised — e.g. by `SendInput` from us
    /// or another app. The engine MUST drop these to avoid feedback.
    pub injected: bool,
    /// Best-effort monotonic timestamp in ms.
    pub timestamp_ms: u64,
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

    /// True if any of Ctrl / Alt / Meta is held — these usually denote
    /// shortcuts, not text entry, so the engine should skip the buffer.
    pub fn is_command(&self) -> bool {
        self.control || self.alt || self.meta
    }
}

/// A single keystroke captured into the word buffer. Stored in
/// scancode-space because scancodes are layout-independent — the
/// engine then translates them through layout-mapping tables to find
/// the produced character under any candidate layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WordKey {
    pub scancode: u32,
    pub shift: bool,
    pub timestamp_ms: u64,
}

/// Input passed to a [`Detector`] (in `poltertype-detect`). Borrowed slice +
/// strings to avoid an allocation per detect-call.
#[derive(Debug, Clone, Copy)]
pub struct DetectionInput<'a> {
    pub current_layout: &'a LayoutId,
    pub candidate_layouts: &'a [LayoutId],
    /// Buffer text rendered under the *current* layout — what the user
    /// actually sees on screen.
    pub current_text: &'a str,
    /// Recent context (a few preceding words). Empty for v0.1.
    pub recent_context: &'a str,
}

/// What a [`Detector`] decided about the buffer.
#[derive(Debug, Clone, PartialEq)]
pub struct DetectionVerdict {
    /// Layout the detector believes the user *intended* to be in.
    pub best_layout: LayoutId,
    /// 0.0 (no idea) – 1.0 (certain).
    pub confidence: f32,
    /// Free-form, for the tray "why was the layout switched?" tooltip.
    pub reason: String,
}
