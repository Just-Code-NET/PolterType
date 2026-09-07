//! One locale's key → text table, layered from every catalog that has
//! something to say about it.

use std::collections::HashMap;
use std::path::Path;

use tracing::{debug, warn};

/// Compared whole when the language changes: a catalog identical to
/// the one already loaded is not worth a redraw.
#[derive(PartialEq)]
pub struct Catalog {
    locale: String,
    entries: HashMap<String, String>,
    /// What could not be read, in the words the Settings window puts
    /// under the language picker.
    ///
    /// A catalog that silently does nothing is the one bug report
    /// nobody can answer: the interface is in English, the file is
    /// right there, and the only account of what happened is a log
    /// line the person who wrote the file has no reason to read
    /// ([#64](https://github.com/Just-Code-NET/PolterType/issues/64)).
    problems: Vec<String>,
}

impl Catalog {
    /// A catalog with no translations — English, or a locale whose
    /// file could not be read. `tr` then always returns its fallback.
    pub fn empty(locale: String) -> Self {
        Self {
            locale,
            entries: HashMap::new(),
            problems: Vec::new(),
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
    /// A file the parser refuses whole is read again a line at a time
    /// rather than dropped, because one bad line in a community
    /// translation should cost that line, not the language.
    fn absorb(&mut self, text: &str, origin: &str, prefix: Option<&str>) -> usize {
        match toml::from_str::<toml::Table>(text) {
            Ok(table) => self.take(&table, origin, prefix),
            Err(e) => self.salvage(text, origin, prefix, &e),
        }
    }

    /// Read a file the parser refused one line at a time, keeping
    /// every line that is a valid entry on its own.
    ///
    /// A catalog is a flat table of one-line entries, so a line is a
    /// whole statement and can be judged alone. What this buys is the
    /// difference between a translation that loses the line somebody
    /// mistyped — `"C:\path"`, the escape every Windows example
    /// invites — and one that loses the language.
    fn salvage(
        &mut self,
        text: &str,
        origin: &str,
        prefix: Option<&str>,
        whole: &toml::de::Error,
    ) -> usize {
        let mut added = 0usize;
        let mut dropped = 0usize;
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            match toml::from_str::<toml::Table>(trimmed) {
                Ok(table) => added += self.take(&table, origin, prefix),
                Err(_) => dropped += 1,
            }
        }
        // The whole-file error is the one worth showing: it names the
        // first line the parser choked on, which is where the fix is.
        // Its Display spans several lines of caret diagram, and a
        // label is one line.
        let detail = whole
            .to_string()
            .lines()
            .next()
            .unwrap_or("invalid TOML")
            .to_owned();
        warn!(%origin, %detail, dropped, added, "UI translation has lines that are not valid TOML");
        self.problems.push(format!("{origin}: {detail}"));
        added
    }

    /// Fold a parsed catalog in.
    ///
    /// Nested tables and non-string values are skipped rather than
    /// rejecting the file: a catalog is flat, so a `[section]` header
    /// somebody added out of habit costs the entries under it and
    /// nothing else.
    fn take(&mut self, table: &toml::Table, origin: &str, prefix: Option<&str>) -> usize {
        let mut added = 0usize;
        let mut blank = 0usize;
        let mut not_text = 0usize;
        self.entries.reserve(table.len());
        for (key, value) in table {
            match value.as_str() {
                Some(s) if !s.trim().is_empty() => {
                    self.entries.insert(namespaced(prefix, key), s.to_owned());
                    added += 1;
                }
                // An empty translation means "not translated yet";
                // storing it would shadow the English fallback with a
                // blank label. Deliberate, and the normal state of a
                // catalog being filled in a line at a time, so it is
                // not something to put in front of the user.
                Some(_) => blank += 1,
                None => not_text += 1,
            }
        }
        if blank > 0 {
            debug!(%origin, blank, "UI translation entries still empty");
        }
        if not_text > 0 {
            warn!(%origin, not_text, "UI translation entries are not text and were skipped");
            self.problems
                .push(format!("{origin}: {not_text} entries are not text"));
        }
        added
    }

    /// What could not be read, oldest source first. Empty is the
    /// normal case, including for a language with no catalog at all —
    /// that is a missing file, not a broken one.
    pub fn problems(&self) -> &[String] {
        &self.problems
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
