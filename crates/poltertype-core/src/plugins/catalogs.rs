//! Which plug-in translations are loaded, and what each is allowed to
//! translate.
//!
//! The two kinds of plug-in ship a catalog for different reasons, and
//! the difference is exactly their difference in trust:
//!
//! * A **pack** is data and cannot run anything. Its `i18n/` is a
//!   translation *of PolterType* — a language the app does not ship,
//!   installed the same way a layout is. Keys are taken as written.
//! * An **extension** ships a program. Its `i18n/` translates **its own
//!   settings pane**, so it is confined to `plugin.<id>.`: whatever the
//!   file says, it cannot reach a label PolterType drew.
//!
//! Both are read after the shipped catalog and before the user's own,
//! so a translation the user wrote themselves still wins.

use std::path::Path;

use crate::i18n::{CatalogSource, I18N_DIR, PLUGIN_NAMESPACE};

use super::discover::plugin_dirs;
use super::enums::PluginKind;
use super::install::{read_header, read_manifest};

/// Every plug-in catalog directory on this machine, in load order.
///
/// Silent about plug-ins that carry no translations: a directory
/// without an `i18n/` is not a problem to report, it is the normal
/// case.
pub fn catalog_sources(data_dir: &Path) -> Vec<CatalogSource> {
    plugin_dirs(data_dir)
        .into_iter()
        .filter_map(|dir| {
            let catalogs = dir.join(I18N_DIR);
            if !catalogs.is_dir() {
                return None;
            }
            let kind = read_header(&dir).ok()?.kind;
            match kind {
                PluginKind::Pack => Some(CatalogSource::open(catalogs)),
                PluginKind::Extension => {
                    // Namespaced by the id in the manifest, never by the
                    // directory name: a plug-in run from a checkout sits
                    // in a directory named after the repository, and its
                    // pane keys are its own all the same.
                    let id = read_manifest(&dir).ok()?.id;
                    let id = id.trim();
                    (!id.is_empty()).then(|| {
                        CatalogSource::confined(catalogs, format!("{PLUGIN_NAMESPACE}.{id}"))
                    })
                }
            }
        })
        .collect()
}
