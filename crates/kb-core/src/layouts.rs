//! Layout mappings — what each Win-SC1 scancode produces under each
//! supported layout. Loaded from `data/layout-mappings/*.toml` baked
//! into the binary at compile time.
//!
//! Adding a new layout = adding a TOML file there + listing it in
//! [`embedded_layouts`].

use std::collections::{HashMap, HashSet};

use kb_detect::{LayoutProfile, Script};
use kb_types::{LayoutId, WordKey};
use serde::Deserialize;
use thiserror::Error;
use tracing::warn;

#[derive(Debug, Error)]
pub enum LayoutLoadError {
    #[error("invalid layout TOML: {0}")]
    Toml(#[from] toml::de::Error),
}

/// Embedded layout descriptions baked into the binary at compile time.
/// Each tuple is `(file-name-for-debug, mapping TOML, optional
/// wordlist)`. The wordlist is one lowercase word per line and feeds
/// the [`DictionaryDetector`](kb_detect::DictionaryDetector).
const fn embedded_layouts() -> &'static [(&'static str, &'static str, Option<&'static str>)] {
    &[
        (
            "en_us.toml",
            include_str!("../../../data/layout-mappings/en_us.toml"),
            Some(include_str!("../../../data/wordlists/en_us.txt")),
        ),
        (
            "uk_ua.toml",
            include_str!("../../../data/layout-mappings/uk_ua.toml"),
            Some(include_str!("../../../data/wordlists/uk_ua.txt")),
        ),
    ]
}

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

#[derive(Debug, Clone)]
pub struct LayoutMapping {
    pub id: LayoutId,
    pub name: String,
    pub script: Script,
    /// scancode → (unshifted, shifted) characters.
    pub keys: HashMap<u32, (char, Option<char>)>,
    /// Pre-computed lower-case vowel set, for the detector profile.
    pub vowels: Vec<char>,
    /// Common-word dictionary (lowercase). The dictionary detector
    /// consults this to decide whether a candidate text is a real
    /// word in the layout's language.
    pub words: HashSet<String>,
}

impl LayoutMapping {
    pub fn from_toml_str(input: &str) -> Result<Self, LayoutLoadError> {
        Self::from_parts(input, None)
    }

    /// Parse the layout TOML and (optionally) a wordlist text file
    /// (one lowercase word per line; blank lines / `#`-comments are
    /// ignored) so the dictionary detector has something to match on.
    pub fn from_parts(toml_input: &str, wordlist: Option<&str>) -> Result<Self, LayoutLoadError> {
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

        let vowels = derive_vowels(script);
        let words = wordlist.map(parse_wordlist).unwrap_or_default();

        Ok(Self {
            id,
            name,
            script,
            keys,
            vowels,
            words,
        })
    }

    /// Translate a single keystroke to the character produced under
    /// this layout, if known.
    pub fn translate_key(&self, key: WordKey) -> Option<char> {
        let (plain, shifted) = self.keys.get(&key.scancode)?;
        if key.shift {
            Some(shifted.unwrap_or(*plain))
        } else {
            Some(*plain)
        }
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

    /// Build the [`LayoutProfile`] used by the detector pipeline.
    pub fn detector_profile(&self) -> LayoutProfile {
        LayoutProfile::new(self.id.clone(), self.script, self.vowels.iter().copied())
    }
}

fn parse_scancode(s: &str) -> Option<u32> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(rest, 16).ok()
    } else {
        s.parse::<u32>().ok()
    }
}

/// Parse a one-word-per-line text file into a lowercase HashSet.
/// Blank lines and `#` comments are skipped.
fn parse_wordlist(input: &str) -> HashSet<String> {
    input
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_lowercase)
        .collect()
}

fn first_char(s: &str) -> Option<char> {
    s.chars().next()
}

/// Default vowel set per script — adjusted in code for languages whose
/// vowels diverge from the script default. Driven by the layout's `id`
/// so that uk-UA and ru-RU (both Cyrillic) get different sets.
fn derive_vowels(script: Script) -> Vec<char> {
    match script {
        Script::Latin => "aeiouy".chars().collect(),
        Script::Cyrillic => "аеиіоуюяєїыэ".chars().collect(),
        Script::Greek => "αεηιουω".chars().collect(),
        Script::Armenian => "աեէիոույ".chars().collect(),
        Script::Hebrew | Script::Arabic | Script::Other => Vec::new(),
    }
}

// ─── LayoutDb: handle to all loaded layouts ──────────────────────────

#[derive(Debug, Clone, Default)]
pub struct LayoutDb {
    by_id: HashMap<LayoutId, LayoutMapping>,
}

impl LayoutDb {
    /// Load every layout embedded in the binary. Loud-but-graceful
    /// on parse errors — we want to see them in the log without
    /// killing the whole app, since another layout may still be
    /// usable.
    pub fn load_embedded() -> Self {
        let mut by_id = HashMap::new();
        for (name, body, wordlist) in embedded_layouts() {
            match LayoutMapping::from_parts(body, *wordlist) {
                Ok(layout) => {
                    by_id.insert(layout.id.clone(), layout);
                }
                Err(e) => {
                    tracing::error!(file = name, err = %e, "failed to load embedded layout");
                }
            }
        }
        Self { by_id }
    }

    pub fn get(&self, id: &LayoutId) -> Option<&LayoutMapping> {
        self.by_id.get(id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &LayoutId> {
        self.by_id.keys()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&LayoutId, &LayoutMapping)> {
        self.by_id.iter()
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_layouts_load() {
        let db = LayoutDb::load_embedded();
        assert!(db.get(&LayoutId::from("en-US")).is_some());
        assert!(db.get(&LayoutId::from("uk-UA")).is_some());
    }

    #[test]
    fn wordlists_loaded_with_layouts() {
        let db = LayoutDb::load_embedded();
        let en = db.get(&LayoutId::from("en-US")).expect("en-US");
        let uk = db.get(&LayoutId::from("uk-UA")).expect("uk-UA");
        // Sanity: a few words we expect on both sides, including
        // single-letter prepositions / pronouns.
        assert!(en.words.contains("the"));
        assert!(en.words.contains("hello"));
        assert!(en.words.contains("a"));
        assert!(en.words.contains("i"));
        assert!(uk.words.contains("що"));
        assert!(uk.words.contains("мені"));
        assert!(uk.words.contains("цим"));
        assert!(uk.words.contains("а"));
        assert!(uk.words.contains("і"));
        assert!(uk.words.contains("у"));
    }

    #[test]
    fn round_trip_hello_through_uk() {
        let db = LayoutDb::load_embedded();
        let en = db.get(&LayoutId::from("en-US")).expect("en-US");
        let uk = db.get(&LayoutId::from("uk-UA")).expect("uk-UA");

        // Scancodes for h, e, l, l, o on a US-ANSI keyboard:
        let buf = vec![
            WordKey {
                scancode: 0x23,
                shift: false,
                timestamp_ms: 0,
            }, // h
            WordKey {
                scancode: 0x12,
                shift: false,
                timestamp_ms: 0,
            }, // e
            WordKey {
                scancode: 0x26,
                shift: false,
                timestamp_ms: 0,
            }, // l
            WordKey {
                scancode: 0x26,
                shift: false,
                timestamp_ms: 0,
            }, // l
            WordKey {
                scancode: 0x18,
                shift: false,
                timestamp_ms: 0,
            }, // o
        ];
        assert_eq!(en.translate_buffer(&buf), "hello");
        assert_eq!(uk.translate_buffer(&buf), "руддщ");
    }

    #[test]
    fn round_trip_pryvit_through_en() {
        let db = LayoutDb::load_embedded();
        let en = db.get(&LayoutId::from("en-US")).expect("en-US");
        let uk = db.get(&LayoutId::from("uk-UA")).expect("uk-UA");

        // Scancodes for п (0x22), р (0x23), и (0x30), в (0x20), і (0x1F), т (0x31)
        let buf = vec![
            WordKey {
                scancode: 0x22,
                shift: false,
                timestamp_ms: 0,
            },
            WordKey {
                scancode: 0x23,
                shift: false,
                timestamp_ms: 0,
            },
            WordKey {
                scancode: 0x30,
                shift: false,
                timestamp_ms: 0,
            },
            WordKey {
                scancode: 0x20,
                shift: false,
                timestamp_ms: 0,
            },
            WordKey {
                scancode: 0x1F,
                shift: false,
                timestamp_ms: 0,
            },
            WordKey {
                scancode: 0x31,
                shift: false,
                timestamp_ms: 0,
            },
        ];
        assert_eq!(uk.translate_buffer(&buf), "привіт");
        // The en-US rendering should be Latin-only too, even if not a word.
        let en_text = en.translate_buffer(&buf);
        assert!(en_text.is_ascii());
    }

    #[test]
    fn shift_picks_uppercase() {
        let db = LayoutDb::load_embedded();
        let en = db.get(&LayoutId::from("en-US")).expect("en-US");
        let buf = vec![WordKey {
            scancode: 0x23,
            shift: true,
            timestamp_ms: 0,
        }];
        assert_eq!(en.translate_buffer(&buf), "H");
    }
}
