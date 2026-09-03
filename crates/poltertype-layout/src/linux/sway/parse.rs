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
pub(crate) fn json_string(line: &str) -> Option<String> {
    let value = match line.find("\": ") {
        Some(colon) => &line[colon + 3..],
        None => line,
    };
    let start = value.find('"')? + 1;
    let rest = &value[start..];
    let end = rest.find('"')?;
    Some(rest[..end].replace("\\/", "/"))
}
