//! One locale's key → text table, layered from every catalog that has
//! something to say about it.

use std::collections::HashMap;
use std::path::Path;

use tracing::warn;

pub struct Catalog {
    locale: String,
    entries: HashMap<String, String>,
}

impl Catalog {
    /// A catalog with no translations — English, or a locale whose
    /// file could not be read. `tr` then always returns its fallback.
    pub fn empty(locale: String) -> Self {
        Self {
            locale,
            entries: HashMap::new(),
        }
    }

    /// One directory's catalog for this locale, taken as written.
    pub fn load(dir: &Path, locale: &str) -> Self {
        let mut catalog = Self::empty(locale.to_owned());
        catalog.overlay(dir, None);
        catalog
    }

    /// One catalog file's text, taken as written.
    pub fn parse(locale: &str, text: &str, origin: &str) -> Self {
        let mut catalog = Self::empty(locale.to_owned());
        catalog.absorb(text, origin, None);
        catalog
    }

    /// Layer `<dir>/<locale>.toml` over what is here already, this file
    /// winning where both name a key, and answer how many entries it
    /// contributed.
    ///
    /// `prefix` confines the file to one namespace: a key already
    /// inside it is kept, any other is moved there. Both forms are
    /// accepted because an author who writes the prefix out is not
    /// making a mistake — they are just repeating themselves.
    pub fn overlay(&mut self, dir: &Path, prefix: Option<&str>) -> usize {
        let Some((text, origin)) = read_file(dir, &self.locale) else {
            return 0;
        };
        self.absorb(&text, &origin, prefix)
    }

    /// Take in one catalog file: a flat table of `key = "text"`.
    ///
    /// Nested tables and non-string values are skipped with a warning
    /// rather than rejecting the whole file — one bad line in a
    /// community translation should cost that line, not the language.
    fn absorb(&mut self, text: &str, origin: &str, prefix: Option<&str>) -> usize {
        let parsed: toml::Value = match toml::from_str(text) {
            Ok(v) => v,
            Err(e) => {
                warn!(%origin, %e, "UI translation is not valid TOML; staying in English");
                return 0;
            }
        };
        let Some(table) = parsed.as_table() else {
            warn!(%origin, "UI translation is not a table; staying in English");
            return 0;
        };

        let mut added = 0usize;
        let mut skipped = 0usize;
        self.entries.reserve(table.len());
        for (key, value) in table {
            match value.as_str() {
                // An empty translation means "not translated yet";
                // storing it would shadow the English fallback with a
                // blank label.
                Some(s) if !s.trim().is_empty() => {
                    self.entries.insert(namespaced(prefix, key), s.to_owned());
                    added += 1;
                }
                _ => skipped += 1,
            }
        }
        if skipped > 0 {
            warn!(%origin, skipped, "UI translation entries skipped (empty or not a string)");
        }
        added
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }

    pub fn locale(&self) -> &str {
        &self.locale
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Read `<dir>/<locale>.toml`, falling back to the bare language
/// subtag: a user with `uk_UA.UTF-8` gets `uk.toml`, which is the
/// common case and saves shipping a file per region.
fn read_file(dir: &Path, locale: &str) -> Option<(String, String)> {
    let bare = locale
        .split(['_', '-', '.'])
        .next()
        .unwrap_or(locale)
        .to_owned();
    for candidate in [locale.to_owned(), bare] {
        let path = dir.join(format!("{candidate}.toml"));
        match std::fs::read_to_string(&path) {
            Ok(text) => return Some((text, path.display().to_string())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                warn!(path = %path.display(), %e, "UI translation unreadable");
                continue;
            }
        }
    }
    None
}

fn namespaced(prefix: Option<&str>, key: &str) -> String {
    match prefix {
        None => key.to_owned(),
        Some(p)
            if key
                .strip_prefix(p)
                .is_some_and(|rest| rest.starts_with('.')) =>
        {
            key.to_owned()
        }
        Some(p) => format!("{p}.{key}"),
    }
}
