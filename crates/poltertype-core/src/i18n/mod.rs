//! UI translation.
//!
//! Every translatable string is fetched with [`tr`], which takes a
//! stable key **and the English text**:
//!
//! ```ignore
//! Text::new(tr("languages.title", "Languages"))
//! ```
//!
//! Passing the English at the call site means it is compiled in and
//! cannot go missing: a catalog that fails to load, an untranslated key,
//! a file a packager forgot — each degrades to readable English rather
//! than a blank button or a raw `languages.title`.
//!
//! Translations live in data like layouts and wordlists:
//! `<root>/i18n/<lang>.toml`, one flat table of `key = "text"`, so
//! adding a language is a file rather than a rebuild. Three kinds of
//! root are read in turn — what PolterType ships, then each installed
//! plug-in, then the user's own config directory — and the last one to
//! name a key wins. An extension's catalog is confined to
//! `plugin.<id>.`, which is what lets it translate its own settings
//! pane into a language the interface itself does not have without
//! being able to reword the interface around it.
//!
//! [`init`] runs once before any widget is built; after that [`tr`] is a
//! hash lookup returning a `&'static str`, so the view function
//! allocates nothing per frame. Calling [`tr`] earlier, or after an
//! `init` that found no catalog, returns the English fallback.

mod catalog;
mod consts;
mod detect;
mod lookup;
mod types;

pub use catalog::Catalog;
pub use consts::{I18N_DIR, LOCALE_ENV, PLUGIN_NAMESPACE, SHIPPED_LOCALES};
pub use detect::resolve_locale;
pub use lookup::{active_catalog, active_locale, build, init, tr, tr_args};
pub use types::CatalogSource;

#[cfg(test)]
mod tests;
