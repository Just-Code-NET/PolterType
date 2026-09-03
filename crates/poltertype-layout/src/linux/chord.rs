//! Reading a desktop's own "switch layout" shortcut.
//!
//! Some desktops accept no other mechanism. GNOME 49 ignores every
//! settings key we can write — `current` and `mru-sources` alike — and
//! moves only for its configured binding; MATE restores its own xkb
//! group within milliseconds of a direct lock but honours the `grp:`
//! toggle. Both measured in the desktop matrix on 2026-08-24, by
//! reading the character the keyboard then produced rather than by
//! asking the desktop what it thought.
//!
//! Parsing only — the pressing is the engine's job, because the emitter
//! is.

use poltertype_types::SwitchChord;

// Win SC Set-1, the scancode space the whole engine speaks.
const SC_SPACE: u32 = 0x39;
const SC_CAPSLOCK: u32 = 0x3A;
const SC_LEFTSHIFT: u32 = 0x2A;
const SC_LEFTALT: u32 = 0x38;
const SC_LEFTCTRL: u32 = 0x1D;

/// A GNOME/GTK accelerator — `<Super>space`, `<Control><Shift>k`.
///
/// The list form gsettings prints (`['<Super>space', 'XF86Keyboard']`)
/// is handled by the caller taking the first entry; media keys like
/// `XF86Keyboard` have no scancode we can name and yield `None`.
pub(crate) fn parse_gtk_accelerator(accel: &str) -> Option<SwitchChord> {
    let mut chord = SwitchChord::default();
    let mut rest = accel.trim();
    while let Some(end) = rest.find('>') {
        if !rest.starts_with('<') {
            break;
        }
        match rest[1..end].to_ascii_lowercase().as_str() {
            "control" | "ctrl" | "primary" => chord.ctrl = true,
            "shift" => chord.shift = true,
            "alt" | "mod1" => chord.alt = true,
            "super" | "meta" | "mod4" => chord.meta = true,
            // A modifier we cannot reproduce means we would send the
            // wrong chord, which is worse than sending none.
            _ => return None,
        }
        rest = &rest[end + 1..];
    }
    chord.scancode = key_name_to_scancode(rest)?;
    Some(chord)
}

/// The handful of key names a layout-switch binding is ever spelled
/// with. Anything else — a letter, a function key, `XF86Keyboard` — is
/// deliberately unhandled: this is not a general accelerator parser,
/// and guessing here would press the wrong key on someone's desktop.
fn key_name_to_scancode(name: &str) -> Option<u32> {
    match name.trim().to_ascii_lowercase().as_str() {
        "space" => Some(SC_SPACE),
        "caps_lock" | "capslock" => Some(SC_CAPSLOCK),
        // A bare modifier chord: the modifiers are the whole shortcut.
        "" => Some(0),
        _ => None,
    }
}

/// An xkb `grp:` option — the toggle every X11 desktop configures its
/// layout switching with.
///
/// `grp:alt_shift_toggle` is Alt held while Shift is pressed, which is
/// exactly a chord with `alt` set and Shift as the key. `grp:caps_toggle`
/// has no modifier at all.
pub(crate) fn parse_xkb_group_option(options: &str) -> Option<SwitchChord> {
    for option in options.split(',') {
        let option = option.trim();
        let Some(name) = option.strip_prefix("grp:") else {
            continue;
        };
        let chord = match name {
            "alt_shift_toggle" => SwitchChord {
                alt: true,
                scancode: SC_LEFTSHIFT,
                ..Default::default()
            },
            "ctrl_shift_toggle" => SwitchChord {
                ctrl: true,
                scancode: SC_LEFTSHIFT,
                ..Default::default()
            },
            "shift_caps_toggle" => SwitchChord {
                shift: true,
                scancode: SC_CAPSLOCK,
                ..Default::default()
            },
            "ctrl_alt_toggle" => SwitchChord {
                ctrl: true,
                scancode: SC_LEFTALT,
                ..Default::default()
            },
            "caps_toggle" => SwitchChord {
                scancode: SC_CAPSLOCK,
                ..Default::default()
            },
            "alt_space_toggle" => SwitchChord {
                alt: true,
                scancode: SC_SPACE,
                ..Default::default()
            },
            "win_space_toggle" => SwitchChord {
                meta: true,
                scancode: SC_SPACE,
                ..Default::default()
            },
            "ctrl_space_toggle" => SwitchChord {
                ctrl: true,
                scancode: SC_SPACE,
                ..Default::default()
            },
            "lctrl_lshift_toggle" => SwitchChord {
                ctrl: true,
                scancode: SC_LEFTSHIFT,
                ..Default::default()
            },
            "lalt_toggle" => SwitchChord {
                scancode: SC_LEFTALT,
                ..Default::default()
            },
            "lctrl_toggle" => SwitchChord {
                scancode: SC_LEFTCTRL,
                ..Default::default()
            },
            // `grp:shifts_toggle` (both Shift keys at once) and the
            // rest are left alone rather than approximated.
            _ => continue,
        };
        return Some(chord);
    }
    None
}

#[cfg(test)]
mod tests;
