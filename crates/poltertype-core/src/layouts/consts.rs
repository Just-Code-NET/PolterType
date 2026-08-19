//! Fixed data the loader is built around.

/// Bundled layout stems shipped in the data directory, and the default
/// discovery list when scanning `<data_dir>/layout-mappings/`.
///
/// Not enumerated at runtime: the bundled set is curated, and directory
/// enumeration is one more failure mode (permissions, non-UTF-8 names)
/// on the startup path.
///
/// Kept in lock-step with `build.rs::LAYOUTS`; a mismatch shows up as a
/// "missing TOML" warning at startup, never silently.
pub const BUNDLED_LAYOUT_STEMS: &[&str] = &[
    "en_us", "uk_ua", "ru_ru", "de_de", "es_es", "fr_fr", "pl_pl", "cs_cz", "el_gr", "he_il",
    "tr_tr", "bg_bg", "it_it", "pt_pt", "pt_br",
];

/// Fewest keys an [`poltertype_types::OsKeymap`] must carry before it
/// may replace a bundled mapping.
///
/// A whole character block is 47–48 keys and every real keyboard fills
/// nearly all of it, so this is a floor under a query that went wrong,
/// not a judgement about unusual layouts.
pub const MIN_OS_KEYMAP_KEYS: usize = 30;
