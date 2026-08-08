//! Fixed data the loader is built around.

/// Bundled layout stems shipped in the data directory. Used as the
/// default discovery list when scanning `<data_dir>/layout-mappings/`
/// — we don't enumerate that directory at runtime because:
///
/// * The bundled set is curated (mappings reviewed against OS docs)
///   while user-side TOMLs are accepted on best-effort.
/// * Filesystem enumeration is one more failure mode (permission,
///   non-UTF-8 names) we don't need on the hot startup path.
///
/// Kept in lock-step with `build.rs::LAYOUTS`. A mismatch shows up
/// as a "missing TOML" warning at startup, never silently.
pub const BUNDLED_LAYOUT_STEMS: &[&str] = &[
    "en_us", "uk_ua", "ru_ru", "de_de", "es_es", "fr_fr", "pl_pl", "cs_cz", "el_gr", "he_il",
    "tr_tr", "bg_bg", "it_it", "pt_pt", "pt_br",
];

/// Fewest keys an [`poltertype_types::OsKeymap`] must carry before we
/// let it replace a bundled mapping.
///
/// A whole character block is 47–48 keys and every real keyboard fills
/// nearly all of it, so this is not a judgement about unusual layouts
/// — it is a floor under a query that went wrong. Anything sparser
/// means the OS answered badly, and a bundled mapping that is right
/// for the wrong variant still beats one that is missing half its
/// alphabet.
pub const MIN_OS_KEYMAP_KEYS: usize = 30;
