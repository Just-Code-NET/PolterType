//! Fixed values the translation loader is built around.

/// Sub-directory of `<data_dir>` holding `<lang>.toml` catalogs.
pub const I18N_DIR: &str = "i18n";

/// Locales with a catalog in the repository, for the Settings
/// language picker and the build script that copies them.
///
/// English is not listed: it is not a catalog, it is the fallback
/// compiled into every `tr` call site, and it works even if this
/// directory is missing entirely.
///
/// A locale absent from this list still *works* — drop a
/// `<config-dir>/poltertype/i18n/<lang>.toml` in and set
/// `[ui].language`. The list is what the picker offers, not what the
/// loader accepts.
pub const SHIPPED_LOCALES: &[(&str, &str)] = &[("uk", "Українська")];
