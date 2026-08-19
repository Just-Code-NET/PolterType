//! Correcting bundled mappings against the keyboards the OS reports.
//!
//! A [`LayoutId`] names a *language*, because that is all the OS layout
//! APIs agree on — but a language is not a keyboard. Windows ships
//! three genuinely different Bulgarian keyboards under the one id
//! `bg-BG`, and `bg_bg.toml` can only describe one: for a user on
//! Bulgarian (Phonetic) it is wrong at 79 of 94 key positions, and
//! nothing errors — the corrections are simply built from a keyboard
//! they do not have.
//!
//! So we ask. `LayoutSwitcher::describe_keymaps` reports what the
//! installed keyboards actually produce, and this module lays that over
//! the bundled tables. The mapping keeps its identity — id, name,
//! script, vowels, dictionary are per-*language* — and only the key
//! table, which is per-*keyboard*, is replaced.
//!
//! ```text
//! bundled  ←  plug-in packs  ←  OS keymaps  ←  user TOMLs
//! ```
//!
//! The OS outranks anything we shipped, because it describes the
//! machine rather than guessing at it. A user TOML still outranks the
//! OS: it is an explicit "I know what my keyboard does", and the escape
//! hatch if this ever reads a keyboard wrong.

use std::collections::{HashMap, HashSet};

use poltertype_types::{LayoutId, OsKeymap};
use tracing::{debug, info, warn};

use super::consts::MIN_OS_KEYMAP_KEYS;
use super::types::LayoutMapping;

/// Replace the key table of every loaded layout the OS could describe.
///
/// Languages we ship no mapping for are skipped: a key table without a
/// dictionary detects nothing, so synthesising a layout from the OS
/// alone would be a different feature rather than a fix.
pub(super) fn apply_os_keymaps(keymaps: &[OsKeymap], by_id: &mut HashMap<LayoutId, LayoutMapping>) {
    let mut claimed: HashSet<&LayoutId> = HashSet::new();

    for km in keymaps {
        let Some(mapping) = by_id.get_mut(&km.id) else {
            continue;
        };

        if km.keys.len() < MIN_OS_KEYMAP_KEYS {
            warn!(
                layout = %km.id,
                variant = %km.variant,
                keys = km.keys.len(),
                "OS described too few keys to be a whole keyboard; keeping the bundled mapping"
            );
            continue;
        }

        // Two keyboards for one language collapse to one id and we can
        // only hold one table. The backend puts the keyboard currently
        // in effect first, so the first is the one to keep.
        if !claimed.insert(&km.id) {
            warn!(
                layout = %km.id,
                variant = %km.variant,
                "another keyboard is already installed for this language; ignoring this one"
            );
            continue;
        }

        let derived: HashMap<u32, (char, Option<char>)> = km
            .keys
            .iter()
            .map(|&(scancode, plain, shift)| (scancode, (plain, shift)))
            .collect();

        // Counted rather than just swapped: a large number here is the
        // signature of the variant problem, and it is the one log line
        // that says which keyboard the user has.
        let replaced = derived
            .iter()
            .filter(|(scancode, produced)| mapping.keys.get(*scancode) != Some(*produced))
            .count();
        let dropped = mapping
            .keys
            .keys()
            .filter(|scancode| !derived.contains_key(*scancode))
            .count();

        info!(
            layout = %km.id,
            variant = %km.variant,
            keys = derived.len(),
            replaced,
            dropped,
            "adopted the OS keymap for this keyboard"
        );

        // Which keys, not just how many: "my `ґ` stopped working" is the
        // shape of the bug report this answers. Safe to log — a layout
        // table is not typed text.
        if replaced > 0 || dropped > 0 {
            let mut changes: Vec<String> = derived
                .iter()
                .filter(|(scancode, produced)| mapping.keys.get(*scancode) != Some(*produced))
                .map(|(scancode, (plain, shift))| {
                    let was = match mapping.keys.get(scancode) {
                        Some((p, s)) => format!("{p}{}", s.map(String::from).unwrap_or_default()),
                        None => "—".to_owned(),
                    };
                    format!("0x{scancode:02X} {was}→{plain}{}", shift.unwrap_or(' '))
                })
                .collect();
            changes.sort_unstable();
            debug!(layout = %km.id, changes = %changes.join(", "), "keys the OS disagreed on");
        }

        mapping.keys = derived;
    }
}
