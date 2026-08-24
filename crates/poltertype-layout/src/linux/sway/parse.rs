//! Pure parsing of `swaymsg -t get_inputs` output. No process spawning
//! here, so the shapes sway actually prints can be pinned in tests.
//!
//! Hand-parsed rather than deserialised, for the same reason the KDE and
//! Hyprland backends are: this crate carries no JSON dependency, and the
//! three fields we want sit one per line in the pretty output `swaymsg`
//! prints without `-r`.

use super::*;
use crate::LayoutId;
use crate::linux::shared::keymap_to_layout;

/// The keyboard sway would apply a layout change to, reduced to the two
/// things we need from it.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct SwayKeyboard {
    pub(crate) layouts: Vec<LayoutId>,
    pub(crate) active: Option<usize>,
}

/// First keyboard that reports more than one layout, else the first
/// keyboard at all.
///
/// Sway lists a keyboard entry for the power button and the video bus
/// as readily as for the keyboard, and those carry the same single
/// default layout. Taking the first entry blind would report one layout
/// on a machine that has two — and one layout is indistinguishable from
/// "nothing to switch to".
pub(crate) fn parse_inputs(out: &str) -> SwayKeyboard {
    let mut best = SwayKeyboard::default();
    for block in split_blocks(out) {
        if !block.contains("\"type\": \"keyboard\"") {
            continue;
        }
        let kb = parse_keyboard(block);
        if kb.layouts.len() > best.layouts.len() {
            best = kb;
        }
    }
    best
}

/// Split on the brace that opens each input object at the top level of
/// the pretty output — two spaces of indent, exactly.
fn split_blocks(out: &str) -> Vec<&str> {
    let mut blocks = Vec::new();
    let mut start = None;
    for (offset, line) in line_offsets(out) {
        if line == "  {" {
            start = Some(offset);
        } else if line == "  }," || line == "  }" {
            if let Some(s) = start.take() {
                blocks.push(&out[s..offset + line.len()]);
            }
        }
    }
    blocks
}

fn line_offsets(out: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut offset = 0;
    out.lines().map(move |line| {
        let here = offset;
        offset += line.len() + 1;
        (here, line)
    })
}

fn parse_keyboard(block: &str) -> SwayKeyboard {
    let mut layouts = Vec::new();
    let mut active = None;
    let mut in_names = false;
    for line in block.lines() {
        let line = line.trim();
        if line.starts_with("\"xkb_layout_names\"") {
            in_names = true;
            continue;
        }
        if in_names {
            if line.starts_with(']') {
                in_names = false;
                continue;
            }
            if let Some(name) = json_string(line) {
                layouts.push(keymap_to_layout(&name));
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("\"xkb_active_layout_index\":") {
            active = rest.trim().trim_end_matches(',').parse::<usize>().ok();
        }
    }
    SwayKeyboard { layouts, active }
}

/// The quoted **value** on the line, with sway's escaping undone — it
/// writes `/` as `\/`, which is legal JSON and appears in every device
/// identifier.
///
/// A line inside an array has no key, so the value is the only quoted
/// run; a line with a key has two, and taking the first would return
/// the key name.
fn json_string(line: &str) -> Option<String> {
    let value = match line.find("\": ") {
        Some(colon) => &line[colon + 3..],
        None => line,
    };
    let start = value.find('"')? + 1;
    let rest = &value[start..];
    let end = rest.find('"')?;
    Some(rest[..end].replace("\\/", "/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verbatim `swaymsg -t get_inputs` from the desktop-matrix guest,
    /// sway 1.11 on Ubuntu 26.04, 2026-08-24 — including the power
    /// button and video bus entries that also call themselves keyboards
    /// and carry one layout each.
    const REAL: &str = r#"[
  {
    "identifier": "2:6:ImExPS\/2_Generic_Explorer_Mouse",
    "name": "ImExPS\/2 Generic Explorer Mouse",
    "type": "pointer",
    "vendor": 2,
    "product": 6
  },
  {
    "identifier": "0:1:Power_Button",
    "name": "Power Button",
    "type": "keyboard",
    "xkb_layout_names": [
      "English (US)"
    ],
    "xkb_active_layout_index": 0,
    "xkb_active_layout_name": "English (US)",
    "vendor": 0,
    "product": 1
  },
  {
    "identifier": "1:1:AT_Translated_Set_2_keyboard",
    "name": "AT Translated Set 2 keyboard",
    "type": "keyboard",
    "repeat_delay": 600,
    "repeat_rate": 25,
    "xkb_layout_names": [
      "English (US)",
      "Russian"
    ],
    "xkb_active_layout_index": 1,
    "xkb_active_layout_name": "Russian",
    "libinput": {
      "send_events": "enabled"
    },
    "vendor": 1,
    "product": 1
  }
]"#;

    #[test]
    fn the_keyboard_with_layouts_wins_over_the_power_button() {
        let kb = parse_inputs(REAL);
        assert_eq!(
            kb.layouts,
            vec![LayoutId::from("en-US"), LayoutId::from("ru-RU")]
        );
        assert_eq!(kb.active, Some(1));
    }

    #[test]
    fn a_session_with_one_layout_reports_one() {
        let one = REAL.replace(
            "      \"English (US)\",\n      \"Russian\"\n",
            "      \"English (US)\"\n",
        );
        let kb = parse_inputs(&one);
        assert_eq!(kb.layouts, vec![LayoutId::from("en-US")]);
    }

    #[test]
    fn no_keyboard_at_all_is_empty_rather_than_a_guess() {
        let kb = parse_inputs("[\n  {\n    \"type\": \"pointer\"\n  }\n]");
        assert!(kb.layouts.is_empty());
        assert_eq!(kb.active, None);
    }

    #[test]
    fn sways_escaped_slashes_survive() {
        assert_eq!(
            json_string(r#"    "name": "ImExPS\/2 Generic Explorer Mouse","#).as_deref(),
            Some("ImExPS/2 Generic Explorer Mouse")
        );
    }
}
