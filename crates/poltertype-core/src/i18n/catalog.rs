//! One locale's key → text table.

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

    /// Read `<dir>/<locale>.toml`, falling back to the bare language
    /// subtag: a user with `uk_UA.UTF-8` gets `uk.toml`, which is the
    /// common case and saves shipping a file per region.
    pub fn load(dir: &Path, locale: &str) -> Self {
        let bare = locale
            .split(['_', '-', '.'])
            .next()
            .unwrap_or(locale)
            .to_owned();
        for candidate in [locale.to_owned(), bare] {
            let path = dir.join(format!("{candidate}.toml"));
            match std::fs::read_to_string(&path) {
                Ok(text) => return Self::parse(locale, &text, &path.display().to_string()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => {
                    warn!(path = %path.display(), %e, "UI translation unreadable");
                    continue;
                }
            }
        }
        Self::empty(locale.to_owned())
    }

    /// Parse one catalog file: a flat table of `key = "text"`.
    ///
    /// Nested tables and non-string values are skipped with a warning
    /// rather than rejecting the whole file — one bad line in a
    /// community translation should cost that line, not the language.
    pub fn parse(locale: &str, text: &str, origin: &str) -> Self {
        let parsed: toml::Value = match toml::from_str(text) {
            Ok(v) => v,
            Err(e) => {
                warn!(%origin, %e, "UI translation is not valid TOML; staying in English");
                return Self::empty(locale.to_owned());
            }
        };
        let Some(table) = parsed.as_table() else {
            warn!(%origin, "UI translation is not a table; staying in English");
            return Self::empty(locale.to_owned());
        };

        let mut entries = HashMap::with_capacity(table.len());
        let mut skipped = 0usize;
        for (key, value) in table {
            match value.as_str() {
                // An empty translation means "not translated yet";
                // storing it would shadow the English fallback with a
                // blank label.
                Some(s) if !s.trim().is_empty() => {
                    entries.insert(key.clone(), s.to_owned());
                }
                _ => skipped += 1,
            }
        }
        if skipped > 0 {
            warn!(%origin, skipped, "UI translation entries skipped (empty or not a string)");
        }
        Self {
            locale: locale.to_owned(),
            entries,
        }
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
