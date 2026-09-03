//! Where the catalogs of one locale are read from.

use std::path::PathBuf;

/// One directory of `<locale>.toml` catalogs, and the namespace its
/// keys may reach.
///
/// A *confined* source contributes only keys under its prefix: a key
/// written without it is moved there rather than refused, so however a
/// third party writes its file, it cannot land on a label PolterType
/// drew itself.
#[derive(Debug, Clone)]
pub struct CatalogSource {
    pub dir: PathBuf,
    /// `None` — anything in the file is taken as written.
    pub prefix: Option<String>,
}

impl CatalogSource {
    /// A catalog trusted with every key: what PolterType ships, and
    /// what the user put in their own config directory.
    pub fn open(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            prefix: None,
        }
    }

    /// A catalog that may only translate its own corner of the
    /// interface.
    pub fn confined(dir: impl Into<PathBuf>, prefix: impl Into<String>) -> Self {
        Self {
            dir: dir.into(),
            prefix: Some(prefix.into()),
        }
    }
}
