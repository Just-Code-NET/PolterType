//! Pure helpers — parsing scraps and derived language data.
//! No I/O here; everything is unit-testable in isolation.

use std::collections::{HashMap, HashSet};

use poltertype_detect::Script;
use poltertype_types::LayoutId;

use super::types::LayoutMapping;

pub fn parse_scancode(s: &str) -> Option<u32> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(rest, 16).ok()
    } else {
        s.parse::<u32>().ok()
    }
}

pub fn first_char(s: &str) -> Option<char> {
    s.chars().next()
}

/// Default vowel set per script — adjusted in code for languages whose
/// vowels diverge from the script default.
pub fn derive_vowels(id: &LayoutId, script: Script) -> Vec<char> {
    match id.as_str() {
        "uk-UA" => "аеиіоуюяєї".chars().collect(),
        "ru-RU" => "аеёиоуыэюя".chars().collect(),
        "de-DE" => "aeiouäöü".chars().collect(),
        "es-ES" => "aeiouáéíóúü".chars().collect(),
        "fr-FR" => "aeiouyàâéèêëîïôûùüÿ".chars().collect(),
        _ => match script {
            Script::Latin => "aeiouy".chars().collect(),
            Script::Cyrillic => "аеиіоуюяєїыэё".chars().collect(),
            Script::Greek => "αεηιουω".chars().collect(),
            Script::Armenian => "աեէիոույ".chars().collect(),
            Script::Hebrew | Script::Arabic | Script::Other => Vec::new(),
        },
    }
}

/// Cheap pre-parse: extract the `id = "..."` line from a layout TOML
/// without paying for the full `toml::from_str`. Returns `None` if
/// the file is malformed in a way that obviously won't parse later
/// either — caller can skip noisily.
///
/// We accept either single- or double-quoted strings, optional
/// surrounding whitespace, and skip `#` comments. This is pure
/// regex-with-discipline territory; the trade-off is that a
/// pathological TOML with `id` inside a multi-line string would
/// confuse us, but every real layout TOML has the id on a top-level
/// line and we'd rather pay 50µs of grep-ish parsing than full TOML
/// parse + clone for layouts we're going to skip anyway.
pub fn peek_layout_id(toml: &str) -> Option<String> {
    for line in toml.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let rest = match line.strip_prefix("id") {
            Some(r) => r.trim_start(),
            None => continue,
        };
        let rest = rest.strip_prefix('=')?.trim_start();
        let (open, close) = if let Some(r) = rest.strip_prefix('"') {
            (r, '"')
        } else if let Some(r) = rest.strip_prefix('\'') {
            (r, '\'')
        } else {
            continue;
        };
        let end = open.find(close)?;
        return Some(open[..end].to_owned());
    }
    None
}

pub fn compute_letter_scancodes(by_id: &HashMap<LayoutId, LayoutMapping>) -> HashSet<(u32, bool)> {
    let mut out = HashSet::new();
    for mapping in by_id.values() {
        for (&sc, (plain, shift)) in &mapping.keys {
            if plain.is_alphabetic() {
                out.insert((sc, false));
            }
            if shift.is_some_and(char::is_alphabetic) {
                out.insert((sc, true));
            }
        }
    }
    out
}

// ─── Tests ───────────────────────────────────────────────────────────
