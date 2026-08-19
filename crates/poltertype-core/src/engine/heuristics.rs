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
/// (terminals), and `Shift+Insert`.
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

/// Bare modifier keys: left/right Ctrl, Shift, Alt, Meta and Caps Lock.
/// The Linux listener emits a modifier's own press with its flag
/// already set, so without this exemption `Ctrl↓` alone reads as a
/// command and abandons the buffer, killing the suggestion-accept chord
/// before its digit arrives. Left-hand codes are SC Set-1, right-hand
/// ones the raw evdev codes.
pub fn is_modifier_scancode(sc: u32) -> bool {
    matches!(
        sc,
        0x1D | 0x2A | 0x36 | 0x38 | 0x3A | 0x61 | 0x64 | 0x7D | 0x7E
    )
}

/// Case-insensitive basename match against the user's disabled-apps
/// list. ASCII-lowercase rather than full Unicode lowering: every
/// executable basename we ever match is ASCII.
pub fn app_is_disabled(exe: &str, disabled: &[String]) -> bool {
    let needle = exe.to_ascii_lowercase();
    disabled
        .iter()
        .any(|entry| entry.eq_ignore_ascii_case(&needle))
}

/// Boundary characters that mean URL / path / email / code rather than
/// prose, and therefore suppress auto-switching.
///
/// Deliberately only characters that are almost never sentence
/// punctuation. `.`, the brackets, `"` and `+ * < > | ~ \`` are left
/// out on purpose: too common in prose to call structural.
pub fn is_structural_boundary(ch: char) -> bool {
    matches!(ch, ':' | '/' | '\\' | '@' | '=' | '#' | '&')
}

/// A boundary that *submits* or *navigates* rather than separating
/// words mid-line. Auto-correction re-emits the boundary after the
/// corrected word, and re-pressing one of these runs a command or moves
/// focus — with the line usually gone, so the replay lands on a fresh
/// prompt as garbage. The manual hotkey still works: `last_word` is
/// stashed before this filter.
pub fn is_submission_boundary(ch: char) -> bool {
    matches!(ch, '\n' | '\r' | '\t')
}

/// True when the rendered word looks like a deliberate ALL-CAPS
/// abbreviation: at least two cased letters, every one uppercase.
///
/// A lone capital (`I`, `A`, `Я`) is ambiguous with a sentence start,
/// hence ≥2. One lowercase letter disqualifies (`iPhone`, `IPv4`).
/// Uncased characters neither help nor hurt, so `URL2` and `DON'T`
/// register while a caseless script never does.
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

/// Decide whether `id` belongs in the candidate set the detectors score
/// against. Three filters, AND'd:
///
/// * **`active`** — empty means no allow-list. The *current* layout is
///   always admitted, so a user typing in a layout they did not
///   whitelist cannot be locked into a Switch verdict.
/// * **`ignored`** — never passes, period.
/// * **`os_active`** — layouts the OS reports as enabled, again with
///   the current one as a safety net. `None` = query failed, fail open.
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

/// Which key reproduces the boundary character `ch` under `target`.
///
/// The separator that closed a word is not part of the mistake, but the
/// replay happens *after* the layout flipped: `Shift`+`0x35` is `,`
/// under uk-UA and `?` under en-US, so replaying it as pressed rewrote
/// the user's punctuation. See `docs/ARCHITECTURE.md` § The correction
/// path.
///
/// The scancode is kept as typed when the target produces the same
/// character anyway, when the key is layout-independent (space, Enter,
/// Tab are in no mapping table), and when the target cannot produce the
/// character at all — the old glyph beats abandoning an otherwise
/// correct fix.
pub fn boundary_key_for(
    layouts: &LayoutDb,
    target: &LayoutId,
    scancode: u32,
    shift: bool,
    ch: char,
) -> (u32, bool) {
    let Some(mapping) = layouts.get(target) else {
        return (scancode, shift);
    };
    let as_typed = mapping.translate_key(poltertype_types::WordKey {
        scancode,
        shift,
        timestamp_ms: 0,
    });
    if as_typed == Some(ch) {
        return (scancode, shift);
    }
    mapping.key_for_char(ch).unwrap_or((scancode, shift))
}

/// Render the buffer through the current layout, skipping every
/// *cross-layout artifact* — punctuation under the current layout whose
/// scancode is a letter somewhere else.
///
/// The dictionary detector strips those before lookup and the code-token
/// guard needs the same courtesy, or it fires on every Ukrainian word
/// containing `ж`, `х`, `ї`, `є`: `Друже` under en-US renders `Lhe;t`,
/// and that `;` made `looks_like_code_token` veto the switch.
///
/// Falls back to `fallback` when the current layout is not in the DB, so
/// the mid-decision path can always continue.
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
        // Shift granularity is critical: without it, 0x0C unshifted
        // being `ß` in de-DE would strip the shifted `_` of `foo_bar`.
        if !c.is_alphabetic() && layouts.is_letter_in_any_layout(k.scancode, k.shift) {
            continue;
        }
        out.push(c);
    }
    out
}
