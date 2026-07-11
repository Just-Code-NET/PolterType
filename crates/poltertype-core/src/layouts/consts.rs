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
pub const BUNDLED_LAYOUT_STEMS: &[&str] = &["en_us", "uk_ua", "ru_ru", "de_de", "es_es", "fr_fr"];
