//! UI translation.
//!
//! The interface was English-only until 0.10.0, which is an odd look
//! on an app whose entire subject is other people's languages.
//!
//! ## The shape, and why this one
//!
//! Every translatable string is fetched with [`tr`], which takes a
//! stable key **and the English text**:
//!
//! ```ignore
//! Text::new(tr("languages.title", "Languages"))
//! ```
//!
//! Passing the English at the call site is the load-bearing choice.
//! It means English is compiled in and cannot go missing: a catalog
//! that fails to load, a key nobody translated, a file a packager
//! forgot — every one of those degrades to readable English rather
//! than to a blank button or a raw `languages.title` staring at the
//! user. It also keeps the source readable; you do not have to open a
//! TOML file to find out what a screen says.
//!
//! Translations themselves live in data, like layouts and wordlists:
//! `<data_dir>/i18n/<lang>.toml`, one flat table of `key = "text"`.
//! Adding a language is a file, not a rebuild — the same promise
//! `docs/ADDING_A_LANGUAGE.md` makes about keyboard layouts.
//!
//! ## Loaded once, read forever
//!
//! [`init`] is called once while the settings window starts, before
//! any widget is built. After that [`tr`] is a hash lookup returning a
//! `&'static str` borrowed from the process-lifetime catalog, so the
//! view function — which runs on every frame — allocates nothing.
//!
//! Calling [`tr`] before [`init`], or after an `init` that found no
//! catalog, returns the English fallback. There is no failure mode
//! that produces a wrong-looking UI, only a less-translated one.

mod catalog;
mod consts;
mod detect;

pub use catalog::Catalog;
pub use consts::{I18N_DIR, SHIPPED_LOCALES};
pub use detect::resolve_locale;

use std::path::Path;
use std::sync::OnceLock;

use tracing::{debug, info};

static CATALOG: OnceLock<Catalog> = OnceLock::new();

/// Load the catalog for `requested` (or the environment's locale when
/// `None`) out of `<data_dir>/i18n/`.
///
/// Idempotent and infallible by design: a missing directory, an
/// unreadable file or a malformed TOML all leave the UI in English,
/// which is a perfectly good outcome and not worth an error path
/// through the window's startup.
pub fn init(data_dir: &Path, requested: Option<&str>) {
    if CATALOG.get().is_some() {
        return;
    }
    let locale = resolve_locale(requested);
    if locale.starts_with("en") {
        debug!(%locale, "UI language is English; no catalog needed");
        let _ = CATALOG.set(Catalog::empty(locale));
        return;
    }
    let catalog = Catalog::load(&data_dir.join(I18N_DIR), &locale);
    if catalog.is_empty() {
        info!(
            %locale,
            "no UI translation found for this locale; the interface stays in English"
        );
    } else {
        info!(%locale, entries = catalog.len(), "UI translation loaded");
    }
    let _ = CATALOG.set(catalog);
}

/// The translated text for `key`, or `english` when there is none.
///
/// Never allocates and never fails. See the module docs for why the
/// English is a parameter rather than a lookup of its own.
pub fn tr(key: &str, english: &'static str) -> &'static str {
    CATALOG.get().and_then(|c| c.get(key)).unwrap_or(english)
}

/// [`tr`] for a string with `{}` placeholders.
///
/// `format!` needs a literal, so an interpolated sentence cannot be
/// translated through the macro — and interpolated sentences are
/// exactly the long explanatory ones that most need translating.
/// Substitution is positional and deliberately dumb: each `{}` in
/// order takes the next argument.
///
/// A translation with **fewer** placeholders than arguments is
/// honoured as written — some languages genuinely need to drop a
/// number — and one with more leaves the extras as literal `{}`
/// rather than panicking. Nothing here can fail; the worst outcome is
/// a sentence that reads oddly, which a translator can see and fix.
pub fn tr_args(key: &str, english: &'static str, args: &[&str]) -> String {
    let template = tr(key, english);
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    let mut next = args.iter();
    while let Some(pos) = rest.find("{}") {
        out.push_str(&rest[..pos]);
        match next.next() {
            Some(arg) => out.push_str(arg),
            None => out.push_str("{}"),
        }
        rest = &rest[pos + 2..];
    }
    out.push_str(rest);
    out
}

/// The locale actually in force, for the About pane and the logs.
pub fn active_locale() -> &'static str {
    CATALOG.get().map_or("en", Catalog::locale)
}

#[cfg(test)]
mod tests;
