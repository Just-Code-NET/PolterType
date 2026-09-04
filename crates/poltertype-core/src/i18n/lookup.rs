//! Loading the catalogs, looking translations up in them, and swapping
//! them when the interface language changes.

use std::path::{Path, PathBuf};
use std::sync::{PoisonError, RwLock};

use tracing::{debug, info};

use super::{Catalog, CatalogSource, I18N_DIR, resolve_locale};

/// The catalog every lookup goes through, `None` until [`init`].
///
/// Behind a lock because the language can change under a running
/// window, and never freed because [`tr`] hands out `&'static str`
/// borrowed from it: a widget built before a swap may still be holding
/// one of those strings. Only a language that actually changed makes
/// another catalog, so the leak is bounded by how often a user picks
/// one — tens of kilobytes each.
static CATALOG: RwLock<Option<&'static Catalog>> = RwLock::new(None);

/// Load the interface language from every place a catalog can live:
/// what PolterType ships under `<data_dir>/i18n/`, then the plug-ins'
/// own, then the user's `<config_dir>/i18n/`.
///
/// Later wins, so a file dropped in by hand overrides the shipped one —
/// which is what makes the edit-and-reload loop of
/// `docs/TRANSLATING_THE_UI.md` possible. Plug-in catalogs sit in the
/// middle: an extension's is confined to its own namespace by
/// [`crate::plugins::catalog_sources`], so it can add a language the
/// interface does not ship without being able to reword the interface.
///
/// Idempotent and infallible by design: a missing directory, an
/// unreadable file or malformed TOML all leave the UI in English, which
/// is a perfectly good outcome. The first call wins — [`reload`] is how
/// the language changes after it.
pub fn init(data_dir: &Path, requested: Option<&str>, plugins: &[CatalogSource]) {
    if active().is_some() {
        return;
    }
    let locale = resolve_locale(requested);
    let catalog = build(&locale, &sources(data_dir, plugins));
    if catalog.is_empty() {
        if locale.starts_with("en") {
            debug!(%locale, "UI language is English; no catalog needed");
        } else {
            info!(
                %locale,
                "no UI translation found for this locale; the interface stays in English"
            );
        }
    } else {
        info!(%locale, entries = catalog.len(), "UI translation loaded");
    }
    let mut slot = CATALOG.write().unwrap_or_else(PoisonError::into_inner);
    if slot.is_none() {
        *slot = Some(Box::leak(Box::new(catalog)));
    }
}

/// Re-read every catalog and swap the result in — the language picked
/// while the app is running — answering whether anything drawn from the
/// old one has to be redrawn.
///
/// The files are read again rather than the locale merely re-resolved,
/// so this is the translator's loop as well: edit a catalog, ask for a
/// reload, see it. `false` means the text is identical to what is
/// already loaded, so nothing is redrawn and nothing is added to the
/// leak [`CATALOG`] describes.
pub fn reload(data_dir: &Path, requested: Option<&str>, plugins: &[CatalogSource]) -> bool {
    let locale = resolve_locale(requested);
    let fresh = build(&locale, &sources(data_dir, plugins));
    let mut slot = CATALOG.write().unwrap_or_else(PoisonError::into_inner);
    if slot.is_some_and(|current| *current == fresh) {
        debug!(%locale, "UI translation re-read; identical to the one loaded");
        return false;
    }
    info!(%locale, entries = fresh.len(), "UI translation swapped");
    *slot = Some(Box::leak(Box::new(fresh)));
    true
}

/// Every catalog directory, in load order: what PolterType ships, the
/// plug-ins', then the user's own.
///
/// Public because the language picker has to offer exactly what the
/// loader would read — a list built from anywhere else would name a
/// language nothing translates, or hide one that is right there.
pub fn sources(data_dir: &Path, plugins: &[CatalogSource]) -> Vec<CatalogSource> {
    let mut sources = Vec::with_capacity(plugins.len() + 2);
    sources.push(CatalogSource::open(data_dir.join(I18N_DIR)));
    sources.extend(plugins.iter().cloned());
    if let Some(dir) = user_dir() {
        sources.push(CatalogSource::open(dir));
    }
    sources
}

/// Fold `sources` into one catalog for `locale`, in order, later
/// winning.
///
/// Separate from [`init`] because everything about layering is checked
/// here, on directories a test owns, without touching the global.
pub fn build(locale: &str, sources: &[CatalogSource]) -> Catalog {
    let mut catalog = Catalog::empty(locale.to_owned());
    for source in sources {
        let added = catalog.overlay(&source.dir, source.prefix.as_deref());
        if added > 0 {
            debug!(dir = %source.dir.display(), added, "UI translations layered");
        }
    }
    catalog
}

/// The translated text for `key`, or `english` when there is none.
///
/// Never allocates and never fails. See the module docs for why the
/// English is a parameter rather than a lookup of its own.
pub fn tr(key: &str, english: &'static str) -> &'static str {
    active().and_then(|c| c.get(key)).unwrap_or(english)
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

/// The loaded catalog, for text that was not compiled in and so cannot
/// carry its English at the call site — a plug-in's manifest. `None`
/// before [`init`], which leaves a plug-in in the language it declared.
pub fn active_catalog() -> Option<&'static Catalog> {
    active()
}

/// The locale actually in force, for the About pane, the logs, and the
/// plug-ins PolterType spawns.
pub fn active_locale() -> &'static str {
    active().map_or("en", Catalog::locale)
}

/// The catalog in force at this instant. A read lock rather than a
/// plain load: [`reload`] can swap it under a frame being drawn.
fn active() -> Option<&'static Catalog> {
    *CATALOG.read().unwrap_or_else(PoisonError::into_inner)
}

/// The user's own catalog directory. Last in the layering, so anything
/// here overrides both PolterType and its plug-ins.
fn user_dir() -> Option<PathBuf> {
    crate::settings::SettingsStore::project_dirs()
        .ok()
        .map(|dirs| dirs.config_dir().join(I18N_DIR))
}
