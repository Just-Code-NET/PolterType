//! Fixed values the translation loader is built around.

/// Sub-directory of a catalog root holding `<lang>.toml` files.
pub const I18N_DIR: &str = "i18n";

/// Namespace an extension's own translations are confined to:
/// `plugin.<id>.<key>`.
pub const PLUGIN_NAMESPACE: &str = "plugin";

/// Environment variable naming the interface locale, handed to every
/// plug-in process so what it prints can be translated too — the one
/// part of a plug-in's pane its manifest cannot declare in advance.
pub const LOCALE_ENV: &str = "POLTERTYPE_LOCALE";

/// Locales with a catalog in the repository, for the Settings language
/// picker and the build script that copies them.
///
/// English is not listed: it is not a catalog but the fallback compiled
/// into every `tr` call site.
///
/// A locale absent from this list still *works* — drop a
/// `<config-dir>/poltertype/i18n/<lang>.toml` in and set
/// `[general].ui_language`. This is what the picker offers, not what
/// the loader accepts.
pub const SHIPPED_LOCALES: &[(&str, &str)] = &[("uk", "Українська")];
