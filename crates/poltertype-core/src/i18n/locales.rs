//! Which languages this machine can actually show the interface in.

use std::collections::BTreeSet;

use super::{CatalogSource, SHIPPED_LOCALES};

/// Every locale a catalog exists for, as (code, name to show), sorted
/// by code.
///
/// Read off disk rather than from [`SHIPPED_LOCALES`], because the
/// loader reads off disk: a translation the user dropped into their
/// config directory, or one a plug-in brought with it, is exactly as
/// selectable as one PolterType ships. A locale nothing has a name for
/// is offered under its own code — better than hiding a file that
/// works.
pub fn installed_locales(sources: &[CatalogSource]) -> Vec<(String, String)> {
    let mut codes: BTreeSet<String> = BTreeSet::new();
    for source in sources {
        let Ok(entries) = std::fs::read_dir(&source.dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "toml") {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if is_locale_code(stem) {
                    codes.insert(stem.to_owned());
                }
            }
        }
    }
    codes
        .into_iter()
        .map(|code| {
            let name = SHIPPED_LOCALES
                .iter()
                .find(|(known, _)| *known == code)
                .map_or_else(|| code.clone(), |(_, name)| (*name).to_owned());
            (code, name)
        })
        .collect()
}

/// `uk`, `pt_BR`, `zh-Hans` — but not `notes` or `README`, so a stray
/// TOML beside the catalogs is not offered as a language.
fn is_locale_code(stem: &str) -> bool {
    let (language, rest) = match stem.split_once(['_', '-']) {
        Some((language, rest)) => (language, Some(rest)),
        None => (stem, None),
    };
    let is_alpha = |s: &str, min: usize, max: usize| {
        (min..=max).contains(&s.len()) && s.chars().all(|c| c.is_ascii_alphabetic())
    };
    is_alpha(language, 2, 3)
        && rest.is_none_or(|rest| {
            (2..=8).contains(&rest.len()) && rest.chars().all(|c| c.is_ascii_alphanumeric())
        })
}
