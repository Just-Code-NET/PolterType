//! Which language a guide opens in.
//!
//! The Setup pane sends the reader to `docs/PERMISSIONS.md` — in
//! English, in a window that is otherwise Ukrainian, German, Spanish
//! or French. Whether a translation of that page exists cannot be
//! asked at the point of clicking: this program makes exactly one
//! network call and it is the updater. So the catalog that translates
//! the window carries the answer too — `"docs.permissions" = "uk"`
//! says `PERMISSIONS.uk.md` is there to be opened, and a catalog that
//! says nothing leaves the link on the English original.
//!
//! `docs/TRANSLATING_THE_UI.md` is the translator's side of this.

use poltertype_core::i18n::tr;

use super::consts::{DOC_LANGUAGE, DOC_SUFFIX, DOC_TAG_MAX, DOCS_PREFIX, TRANSLATED_DOCS};

/// What to open for `url`: the translation the active catalog claims,
/// or `url` itself — which is every link that is not one of our
/// guides, and every guide nobody has translated.
pub(super) fn localized(url: &str) -> String {
    url.strip_prefix(DOCS_PREFIX)
        .and_then(|file| {
            TRANSLATED_DOCS
                .iter()
                .find(|(name, _)| *name == file)
                .and_then(|(name, key)| in_language(name, tr(key, DOC_LANGUAGE)))
        })
        .unwrap_or_else(|| url.to_owned())
}

/// `PERMISSIONS.md` in `uk` → the URL of `PERMISSIONS.uk.md`.
///
/// `None` for English, and for anything that is not a language tag: a
/// catalog is a file anyone can drop into the config directory, and
/// the one thing it must not be able to do is choose the path a button
/// in this window opens.
fn in_language(file: &str, tag: &str) -> Option<String> {
    if tag == DOC_LANGUAGE || !is_language_tag(tag) {
        return None;
    }
    let stem = file.strip_suffix(DOC_SUFFIX)?;
    Some(format!("{DOCS_PREFIX}{stem}.{tag}{DOC_SUFFIX}"))
}

fn is_language_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= DOC_TAG_MAX
        && tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

#[cfg(test)]
mod tests;
