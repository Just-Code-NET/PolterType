//! Loading the catalog once, and looking translations up in it.

use std::path::Path;
use std::sync::OnceLock;

use tracing::{debug, info};

use super::{Catalog, I18N_DIR, resolve_locale};

static CATALOG: OnceLock<Catalog> = OnceLock::new();

/// Load the catalog for `requested` (or the environment's locale when
/// `None`) out of `<data_dir>/i18n/`.
///
/// Idempotent and infallible by design: a missing directory, an
/// unreadable file or malformed TOML all leave the UI in English, which
/// is a perfectly good outcome.
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
/// translated through the macro. Substitution is positional and
/// deliberately dumb: a translation with **fewer** placeholders than
/// arguments is honoured as written, since some languages genuinely need
/// to drop a number, and one with more leaves the extras literal rather
/// than panicking.
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
