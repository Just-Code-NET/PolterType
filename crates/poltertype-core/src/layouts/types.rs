//! Data shapes: the raw TOML schema, the in-memory
//! [`LayoutMapping`], plug-in manifests, and loader options.

use std::collections::HashMap;
use std::path::Path;

use poltertype_detect::{LayoutDictionary, LayoutProfile, Script};
use poltertype_types::{LayoutId, WordKey};
use serde::Deserialize;
use tracing::warn;

use super::enums::LayoutLoadError;
use super::helpers::{derive_vowels, first_char, parse_scancode};

// ─── Raw TOML schema ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct LayoutToml {
    id: String,
    #[serde(default)]
    name: Option<String>,
    script: Script,
    keys: HashMap<String, KeyMapping>,
}

#[derive(Debug, Deserialize)]
struct KeyMapping {
    plain: String,
    #[serde(default)]
    shift: Option<String>,
}

// ─── In-memory representation ────────────────────────────────────────

#[derive(Clone)]
pub struct LayoutMapping {
    pub id: LayoutId,
    pub name: String,
    pub script: Script,
    /// scancode → (unshifted, shifted) characters.
    pub keys: HashMap<u32, (char, Option<char>)>,
    /// Pre-computed lower-case vowel set, for the detector profile.
    pub vowels: Vec<char>,
    /// Per-layout dictionary (compact FST + optional user-overlay).
    /// `None` means we have no dictionary for this layout — the
    /// detector falls through to plausibility-only.
    pub dictionary: Option<LayoutDictionary>,
}

impl std::fmt::Debug for LayoutMapping {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LayoutMapping")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("script", &self.script)
            .field("keys_n", &self.keys.len())
            .field("dictionary_present", &self.dictionary.is_some())
            .finish()
    }
}

impl LayoutMapping {
    pub fn from_toml_str(input: &str) -> Result<Self, LayoutLoadError> {
        Self::from_parts(input, None)
    }

    /// Parse the layout TOML; bind an optional dictionary (FST blob +
    /// user overlay) to the resulting mapping.
    pub fn from_parts(
        toml_input: &str,
        dictionary: Option<LayoutDictionary>,
    ) -> Result<Self, LayoutLoadError> {
        let raw: LayoutToml = toml::from_str(toml_input)?;
        let id = LayoutId::new(raw.id);
        let name = raw.name.unwrap_or_else(|| id.as_str().to_owned());
        let script = raw.script;

        let mut keys = HashMap::with_capacity(raw.keys.len());
        for (k, v) in raw.keys {
            let Some(scancode) = parse_scancode(&k) else {
                warn!(key = %k, layout = %id, "ignoring unparseable scancode");
                continue;
            };
            let plain = first_char(&v.plain);
            let shift = v.shift.as_deref().and_then(first_char);
            if let Some(plain) = plain {
                keys.insert(scancode, (plain, shift));
            }
        }

        let vowels = derive_vowels(&id, script);

        Ok(Self {
            id,
            name,
            script,
            keys,
            vowels,
            dictionary,
        })
    }

    /// Translate a single keystroke to the character produced under
    /// this layout, if known.
    ///
    /// Caps Lock is applied the way xkb applies it, which is not
    /// "another Shift": on a letter it selects the shifted level and
    /// a held Shift then cancels it back to the base one; on digits
    /// and punctuation it does nothing at all. Folding it into
    /// `shift` at the listener made `1` under Caps Lock read as `!`.
    pub fn translate_key(&self, key: WordKey) -> Option<char> {
        let (plain, shifted) = self.keys.get(&key.scancode)?;
        let caps_applies = key.caps && plain.is_alphabetic();
        if key.shift ^ caps_applies {
            Some(shifted.unwrap_or(*plain))
        } else {
            Some(*plain)
        }
    }

    /// Reverse lookup: which physical key and shift state produces `ch`
    /// under this layout? Used by the suggestion-accept path to type a
    /// replacement as scancodes. Linear over the ~48-key table, a
    /// handful of times per accepted word; an unshifted match wins.
    ///
    /// Ties break on the lowest scancode: a real keyboard carries the
    /// same character on two keys often enough (en-US has `\` on both
    /// `0x2B` and the ISO `0x56`), and iterating the `HashMap` would
    /// pick a different one run to run.
    pub fn key_for_char(&self, ch: char) -> Option<(u32, bool)> {
        let mut plain_hit: Option<u32> = None;
        let mut shifted_hit: Option<u32> = None;
        for (&sc, &(plain, shift)) in &self.keys {
            if plain == ch {
                plain_hit = Some(plain_hit.map_or(sc, |best| best.min(sc)));
            } else if shift == Some(ch) {
                shifted_hit = Some(shifted_hit.map_or(sc, |best| best.min(sc)));
            }
        }
        plain_hit
            .map(|sc| (sc, false))
            .or(shifted_hit.map(|sc| (sc, true)))
    }

    /// Which physical key and **Shift state** to press to make `ch`
    /// appear, given whether Caps Lock is latched right now.
    ///
    /// [`Self::key_for_char`] answers in rendering terms — the level
    /// the character sits on. Pressing that as a modifier while the
    /// lock is on types the opposite case, because xkb applies the
    /// lock a second time on the way out.
    pub fn press_for_char(&self, ch: char, caps_on: bool) -> Option<(u32, bool)> {
        let (scancode, level_shift) = self.key_for_char(ch)?;
        let caps_applies = caps_on
            && self
                .keys
                .get(&scancode)
                .is_some_and(|(plain, _)| plain.is_alphabetic());
        Some((scancode, level_shift ^ caps_applies))
    }

    /// Translate an entire word buffer into a string under this layout.
    /// Untranslatable keystrokes are dropped silently.
    pub fn translate_buffer(&self, keys: &[WordKey]) -> String {
        let mut out = String::with_capacity(keys.len());
        for &k in keys {
            if let Some(c) = self.translate_key(k) {
                out.push(c);
            }
        }
        out
    }

    /// Re-render `text` as if the same keys had been pressed under
    /// `to` — the word-level correction generalised to arbitrary text,
    /// for converting a *selection* (issue #32).
    ///
    /// A character this layout does not carry is passed through
    /// unchanged rather than dropped: a selection is prose, not a word
    /// buffer, and it holds spaces, newlines and punctuation that live
    /// on neither layout. Dropping them would silently reflow the
    /// user's text while claiming to have changed its layout.
    ///
    /// `None` when nothing at all changed, which is the caller's cue
    /// that this selection was not wrong-layout text and should be put
    /// back untouched.
    pub fn transliterate_to(&self, text: &str, to: &Self) -> Option<String> {
        let mut out = String::with_capacity(text.len());
        let mut changed = false;
        for ch in text.chars() {
            let mapped = self.key_for_char(ch).and_then(|(scancode, shift)| {
                to.translate_key(WordKey {
                    scancode,
                    shift,
                    // The lock is a property of the keyboard right now,
                    // not of text that was typed at some point in the
                    // past. Shift comes from the character's own level.
                    caps: false,
                    timestamp_ms: 0,
                })
            });
            match mapped {
                Some(c) => {
                    changed |= c != ch;
                    out.push(c);
                }
                None => out.push(ch),
            }
        }
        changed.then_some(out)
    }

    /// Build the [`LayoutProfile`] used by the detector pipeline.
    pub fn detector_profile(&self) -> LayoutProfile {
        LayoutProfile::new(self.id.clone(), self.script, self.vowels.iter().copied())
    }
}

/// Manifest schema for a plug-in language pack. Lives at
/// `<data_dir>/plugins/<dir-name>/manifest.toml`. Mirrors the contract
/// documented in `docs/DATA_LAYOUT.md`.
///
/// Every field is `#[serde(default)]`, so a manifest written for a
/// newer schema still parses here.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct PluginManifest {
    /// Pack identifier (NOT a layout id — it's the pack's own name,
    /// for log messages and conflict diagnostics).
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// Pack version string. Free-form; we don't enforce semver yet.
    pub version: String,
    /// Optional self-declared layouts. Currently only used for log
    /// output; the loader actually enumerates the pack's
    /// `layout-mappings/` directory regardless.
    #[serde(default)]
    pub supported_layouts: Vec<String>,
}

/// Configuration for [`LayoutDb::load`].
#[derive(Debug, Clone, Default)]
pub struct LoadOptions<'a> {
    /// Directory holding the bundled `layout-mappings/` and
    /// `wordlists/` trees. `None` → resolve via [`crate::data_dir`].
    pub data_dir: Option<&'a Path>,
    /// If `Some(list)`, only load layouts whose id is in `list`.
    /// `None` loads every bundled stem (used by tests and as a
    /// fail-open fallback when the OS query for active layouts
    /// fails).
    pub active_filter: Option<&'a [LayoutId]>,
    /// `<config-dir>/poltertype/layouts/` — user-supplied TOMLs.
    pub user_layout_dir: Option<&'a Path>,
    /// `<config-dir>/poltertype/wordlists/` — user wordlist overlays.
    pub user_wordlist_dir: Option<&'a Path>,
    /// What the OS says the user's keyboards actually produce —
    /// typically `LayoutSwitcher::describe_keymaps()`. Each entry
    /// replaces the key table of the layout it names, because a
    /// [`LayoutId`] is a language and a language is not a keyboard.
    /// `None` (or an empty slice) leaves the bundled tables alone.
    /// See [`super::os_keymap`].
    pub os_keymaps: Option<&'a [poltertype_types::OsKeymap]>,
}
