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
        "pl-PL" => "aeiouyąęó".chars().collect(),
        "cs-CZ" => "aeiouyáéíóúůýě".chars().collect(),
        // Greek needs the accented forms spelled out: the script
        // default covers the bare vowels only, and Greek marks the
        // stressed vowel of almost every polysyllabic word.
        "el-GR" => "αεηιουωάέήίόύώϊϋΐΰ".chars().collect(),
        // `ı` (dotless i) and `ı`'s dotted twin are both vowels, as
        // are ö and ü — the bare Latin default would score four of
        // Turkish's eight vowels as consonants.
        "tr-TR" => "aeıioöuü".chars().collect(),
        // `ъ` is a full vowel in Bulgarian (unlike Russian, where the
        // same glyph is a silent sign), so the generic Cyrillic set
        // is wrong here in both directions.
        "bg-BG" => "аеиоуъюя".chars().collect(),
        "it-IT" => "aeiouàèéìíîòóùú".chars().collect(),
        "pt-PT" | "pt-BR" => "aeiouáàâãéêíóôõúü".chars().collect(),
        _ => match script {
            Script::Latin => "aeiouy".chars().collect(),
            Script::Cyrillic => "аеиіоуюяєїыэё".chars().collect(),
            Script::Greek => "αεηιουω".chars().collect(),
            Script::Armenian => "աեէիոույ".chars().collect(),
            Script::Hebrew | Script::Arabic | Script::Other => Vec::new(),
        },
    }
}

/// Cheap pre-parse: pull the `id = "..."` line out of a layout TOML
/// without paying for a full `toml::from_str`. `None` when the file is
/// malformed in a way that would not parse later either.
///
/// Accepts either quote style, tolerates whitespace and skips `#`
/// comments. An `id` inside a multi-line string would confuse it, but
/// every real layout TOML has it on a top-level line, and this runs on
/// layouts we are about to skip anyway.
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
