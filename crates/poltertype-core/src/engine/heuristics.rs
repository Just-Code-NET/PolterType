//! Pure decision helpers, split out of the engine so each is
//! unit-testable without constructing a full `SwitcherEngine`.

use poltertype_input::{KeyDirection, KeyEvent};
use poltertype_layout::LayoutId;

use crate::layouts::LayoutDb;

use super::consts::{SC_INSERT, SC_V};
use super::types::Chord;

/// Returns `true` exactly once per physical press of `chord`'s key while
/// the chord's modifiers are held. `key_down` carries the latch state
/// across calls.
pub fn match_chord(ev: &KeyEvent, chord: Chord, key_down: &mut bool) -> bool {
    if ev.scancode != chord.scancode {
        return false;
    }
    match ev.direction {
        KeyDirection::Release => {
            *key_down = false;
            false
        }
        KeyDirection::Press => {
            if *key_down {
                return false; // autorepeat — already handled this press
            }
            *key_down = true;
            ev.modifiers.control == chord.ctrl
                && ev.modifiers.shift == chord.shift
                && ev.modifiers.alt == chord.alt
                && ev.modifiers.meta == chord.meta
        }
    }
}

/// True for the common clipboard-paste chords: `Ctrl+V`, `Ctrl+Shift+V`
/// (terminals), and `Shift+Insert`. We only need the press edge.
pub fn is_paste_shortcut(ev: &KeyEvent) -> bool {
    if ev.direction != KeyDirection::Press {
        return false;
    }
    let m = ev.modifiers;
    (m.control && !m.alt && !m.meta && ev.scancode == SC_V)
        || (m.shift && !m.control && !m.alt && !m.meta && ev.scancode == SC_INSERT)
}

/// Scancodes whose replay would submit a line / move focus (Enter,
/// Tab, numpad Enter) — never safe to re-emit as part of a correction.
pub fn is_submission_scancode(sc: u32) -> bool {
    matches!(sc, 0x1C | 0x0F | 0x60)
}

/// Bare modifier keys: left/right Ctrl, Shift, Alt, Meta, plus Caps
/// Lock. A modifier's *own* press/release can never edit text, but on
/// the Linux listener the event already carries its modifier flag
/// (state is updated before the event is emitted) — so without this
/// exemption `Ctrl↓` alone reads as a "command" and abandons the
/// buffer, killing the suggestion-accept chord before its digit ever
/// arrives (and needlessly tainting mid-flight words on a stray Ctrl
/// tap). Left-hand codes are SC Set-1; right-hand ones are the raw
/// evdev codes the listener forwards for extended keys.
pub fn is_modifier_scancode(sc: u32) -> bool {
    matches!(
        sc,
        0x1D | 0x2A | 0x36 | 0x38 | 0x3A | 0x61 | 0x64 | 0x7D | 0x7E
    )
}

/// Case-insensitive basename match against the user's disabled-apps
/// list. We use ASCII-lowercase rather than full Unicode lowering
/// because every executable basename we ever match is ASCII.
pub fn app_is_disabled(exe: &str, disabled: &[String]) -> bool {
    let needle = exe.to_ascii_lowercase();
    disabled
        .iter()
        .any(|entry| entry.eq_ignore_ascii_case(&needle))
}

/// Boundary characters that strongly suggest the user is typing a
/// URL / file path / email address / config expression / source code
/// rather than prose. When the engine sees one of these as the
/// boundary it skips auto-switching: the just-completed token is
/// almost certainly part of an address-like construct and shouldn't
/// be re-rendered through another keyboard layout.
///
/// The list is conservative — only characters that are *almost
/// always* structural in real prose, never sentence punctuation:
///
/// * `:` — URL scheme, time, key:value, ratio, ternary
/// * `/` — path separator, URL, division, regex
/// * `\` — Windows path, escape
/// * `@` — email, mention, decorator, npm scope
/// * `=` — assignment, query string, equality
/// * `#` — anchor, hashtag, source comment, channel
/// * `&` — URL query separator, bitwise
///
/// Notably absent: `.` (also sentence-end), `(`, `)`, `[`, `]`,
/// `{`, `}`, `"` (all common in prose), `+`, `*`, `<`, `>`, `|`,
/// `~`, `` ` `` (less common in prose but lower confidence as
/// "definitely structural").
pub fn is_structural_boundary(ch: char) -> bool {
    matches!(ch, ':' | '/' | '\\' | '@' | '=' | '#' | '&')
}

/// A boundary that *submits* or *navigates* rather than separating words
/// mid-line: Enter/Return, Tab. Auto-correction re-emits the boundary
/// key after the corrected word, and re-pressing one of these is
/// destructive — in a terminal it runs a command, in a chat app it sends
/// the message, with Tab it triggers completion / moves focus. By the
/// time our (necessarily delayed) correction fires the line has usually
/// already been submitted, so the replay also lands on a fresh prompt as
/// garbage. We therefore never auto-correct on these boundaries. The
/// manual switch-last hotkey still works — `last_word` is stashed before
/// this filter runs.
pub fn is_submission_boundary(ch: char) -> bool {
    matches!(ch, '\n' | '\r' | '\t')
}

/// True when the rendered word looks like a deliberate ALL-CAPS
/// abbreviation: at least two cased letters and every cased letter is
/// uppercase. Lone capital letters (`I`, `A`, `Я`) are ambiguous — they
/// match "first letter of a sentence the user just hit Shift for" too —
/// so we require ≥2 to fire.
///
/// Mixed case (`iPhone`, `IPv4`, `Hello`) returns `false`: a single
/// lowercase letter is enough to disqualify, because real ALL-CAPS input
/// has no lowercase by definition. Digits and apostrophes are skipped —
/// `URL2` and `DON'T` still register as ALL CAPS.
///
/// Uncased characters (CJK, digits, punctuation) neither help nor hurt:
/// they're skipped. A word that contains *only* uncased letters cannot
/// be ALL CAPS (no upper-letter count → fails the ≥2 check), so the
/// function returns `false` — which is the right call for languages
/// without case distinction.
pub fn looks_like_all_caps(text: &str) -> bool {
    let mut upper_letters = 0usize;
    for c in text.chars() {
        if c.is_lowercase() {
            return false;
        }
        if c.is_uppercase() {
            upper_letters += 1;
        }
    }
    upper_letters >= 2
}

/// Decide whether `id` belongs in the candidate set the detectors get
/// to score against. Three filters, AND'd together:
///
/// * **Settings allow-list (`active`)** — empty means "no allow-list,
///   every loaded layout passes". Non-empty means only listed layouts
///   pass; the *current* layout always passes regardless, so a Switch
///   verdict is never silently locked-in by virtue of the user typing
///   in a layout they haven't whitelisted.
/// * **Settings veto (`ignored`)** — anything in this list never
///   passes, period.
/// * **OS-active list (`os_active`)** — `Some(list)` means "filter to
///   only layouts the OS reports as currently installed/enabled" (with
///   the current layout always passing as a safety net for the rare
///   case where the OS list omits it transiently). `None` means the
///   query failed and we fail-open — same behaviour as before this
///   filter existed.
///
/// Pulled out as a standalone fn so it's unit-testable without
/// constructing a full engine.
pub fn is_layout_eligible(
    id: &LayoutId,
    current: &LayoutId,
    settings_active: &[LayoutId],
    settings_ignored: &[LayoutId],
    os_active: Option<&[LayoutId]>,
) -> bool {
    let allowed = settings_active.is_empty() || settings_active.contains(id) || id == current;
    let blocked = settings_ignored.contains(id);
    let os_ok = os_active
        .map(|a| a.contains(id) || id == current)
        .unwrap_or(true);
    allowed && !blocked && os_ok
}

/// Render the buffer through the current layout, but skip every
/// character that's a *cross-layout artifact* — i.e. punctuation
/// under the current layout whose scancode is actually a letter
/// somewhere else.
///
/// Why: with the cross-layout-letter buffer hint (see
/// `WordBuffer::feed`), a buffer can contain scancodes whose current-
/// layout rendering is `;` / `[` / `'` even though the user clearly
/// meant a Cyrillic letter. The dictionary detector strips those
/// before lookup; the code-token guard needs the same courtesy or it
/// fires on every Ukrainian word containing `ж`, `х`, `ї`, `є`, etc.
/// (their scancodes are punctuation in en-US: 0x27 → `;`, 0x1A → `[`,
/// 0x28 → `'`, 0x1B → `]`). The visible bug: typing `Друже` under
/// en-US rendered as `Lhe;t`, and the `;` made
/// `looks_like_code_token` veto the auto-switch.
///
/// Falls back to the already-computed `current_text` if the current
/// layout isn't loaded in the DB (shouldn't happen at runtime, but the
/// engine's mid-decision path needs to keep going either way).
pub fn render_for_code_check(
    keys: &[poltertype_types::WordKey],
    current_layout: &LayoutId,
    layouts: &LayoutDb,
    fallback: &str,
) -> String {
    let Some(mapping) = layouts.get(current_layout) else {
        return fallback.to_owned();
    };
    let mut out = String::with_capacity(keys.len());
    for &k in keys {
        let Some(c) = mapping.translate_key(k) else {
            continue;
        };
        // Cross-layout artifact: non-letter under current, but the
        // scancode-at-this-shift IS a letter in some other layout.
        // The user meant a letter, not punctuation — drop it from the
        // code-token view. Checking shift granularity is critical:
        // without it, scancode 0x0C unshifted being `ß` in de-DE
        // would (wrongly) cause the SHIFTED `_` produced under en-US
        // to be stripped from `foo_bar`.
        if !c.is_alphabetic() && layouts.is_letter_in_any_layout(k.scancode, k.shift) {
            continue;
        }
        out.push(c);
    }
    out
}
