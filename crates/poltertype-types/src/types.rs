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
/// agree on — but a language is not a keyboard: Bulgarian alone ships
/// three genuinely different Windows keyboards under `bg-BG`.
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
    /// True if *we* synthesised the event — a correction's own
    /// keystrokes coming back through the listener. The engine MUST
    /// drop these or it corrects its own replay.
    ///
    /// Deliberately not "synthetic by anyone": another tool's injected
    /// keys are the user typing, through a KM switch or an on-screen
    /// keyboard, and dropping those makes the app silent for them.
    /// Where a remapper strips the marker, the engine's expected-echo
    /// queue is the second line of defence.
    pub injected: bool,
    /// Best-effort monotonic timestamp in ms.
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    /// A Shift key physically down — never Caps Lock. The two are not
    /// interchangeable: xkb gives Shift+Lock on a letter the *base*
    /// level (`Caps` + `Shift` types lowercase) and ignores Lock
    /// entirely on digits and punctuation, so a replay that presses
    /// Shift for a character Caps Lock alone produced comes out
    /// lower-case, or as the shifted symbol. See `caps`.
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub meta: bool,
    /// Caps Lock **latched** at the time of the event — the OS lock
    /// state, not a count of how often the key was pressed. The key
    /// is routinely repurposed (`caps:escape`, `grp:caps_toggle`,
    /// `caps:ctrl_modifier`), and then it never latches anything.
    pub caps: bool,
}

impl Modifiers {
    pub const NONE: Self = Self {
        shift: false,
        control: false,
        alt: false,
        meta: false,
        caps: false,
    };

    /// No modifier *held*. Caps Lock is a latch, not a held key, and
    /// chords are never written against it — so it is deliberately
    /// not counted here or in [`Self::is_command`].
    pub fn is_empty(&self) -> bool {
        !(self.shift || self.control || self.alt || self.meta)
    }

    /// True if any of Ctrl / Alt / Meta is held — these usually denote
    /// shortcuts, not text entry, so the engine should skip the buffer.
    pub fn is_command(&self) -> bool {
        self.control || self.alt || self.meta
    }
}

/// The key combination a desktop binds to "switch to the next keyboard
/// layout" — the only mechanism some of them will accept.
///
/// GNOME 49 ignores every settings key we can write and moves only for
/// `<Super>space`; MATE restores its own xkb group within milliseconds
/// of a direct lock but honours `Alt+Shift`. Both were measured in the
/// desktop matrix on 2026-08-24, by reading the character the keyboard
/// then produced.
///
/// It cycles rather than selects, so reaching a particular layout means
/// pressing it and checking, not computing an index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SwitchChord {
    /// Win SC Set-1, as everywhere else in the engine.
    pub scancode: u32,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
}

/// A single keystroke captured into the word buffer. Stored in
/// scancode-space because scancodes are layout-independent — the
/// engine then translates them through layout-mapping tables to find
/// the produced character under any candidate layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WordKey {
    pub scancode: u32,
    /// Shift physically held for this keystroke — the modifier a
    /// replay has to reproduce, and nothing else.
    pub shift: bool,
    /// Caps Lock latched for this keystroke. Only the *rendering*
    /// reads it (a letter typed under Caps Lock appears upper-case);
    /// the replay must not, because the lock is still on when the
    /// keystroke goes back out.
    pub caps: bool,
    pub timestamp_ms: u64,
}

/// Input passed to a [`Detector`] (in `poltertype-detect`). Borrowed to
/// avoid an allocation per detect-call.
#[derive(Debug, Clone, Copy)]
pub struct DetectionInput<'a> {
    pub current_layout: &'a LayoutId,
    pub candidate_layouts: &'a [LayoutId],
    /// Buffer text rendered under the *current* layout — what the user
    /// actually sees on screen.
    pub current_text: &'a str,
    /// Recent context (a few preceding words). Always empty today —
    /// nothing populates it yet.
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
