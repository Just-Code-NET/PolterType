//! Translating what a plug-in's manifest says, before any of it is
//! drawn.
//!
//! A plug-in carries its own `i18n/<locale>.toml`, confined to
//! `plugin.<id>.` — so its author can offer the pane in a language
//! PolterType does not ship, and still cannot touch a word PolterType
//! wrote. This is where that file meets the manifest.
//!
//! Keys are **derived from the manifest's own structure** rather than
//! declared in it: nothing extra to write, and nothing to keep in step
//! when a control moves. [`strings`] prints the derivation for a whole
//! manifest, so a translator never has to work one out.
//!
//! Only what is read is translated. An option's `value`, a control's
//! `key` and a command's `id` are what gets written to the plug-in's
//! config or handed to its program, and stay exactly as its author
//! wrote them.

use crate::i18n::{Catalog, PLUGIN_NAMESPACE};

use super::enums::PaneOption;
use super::types::{ExtensionManifest, PaneControl};

/// How much of a label becomes a key when it has nothing better to be
/// named after. Long enough to stay readable, short enough to type.
const SLUG_MAX: usize = 32;

/// Translate `manifest` in place, using the catalog [`crate::i18n::init`]
/// loaded. A no-op before `init`, or in English.
pub fn localise(manifest: &mut ExtensionManifest, id: &str) {
    if let Some(catalog) = crate::i18n::active_catalog() {
        localise_with(manifest, id, catalog);
    }
}

/// The same against a catalog the caller loaded.
pub fn localise_with(manifest: &mut ExtensionManifest, id: &str, catalog: &Catalog) {
    let prefix = format!("{PLUGIN_NAMESPACE}.{}", id.trim());
    visit(manifest, &mut |key, _english| {
        catalog.get(&format!("{prefix}.{key}")).map(str::to_owned)
    });
}

/// Every string a translation can reach — the settings pane and the
/// tray entries — as key/English pairs in manifest order, the file a
/// translator starts from. Printed by
/// `poltertype --plugin-strings <id>`.
///
/// Keys are relative to the plug-in's own namespace, which is how they
/// are written in its catalog.
pub fn strings(manifest: &ExtensionManifest) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut scratch = manifest.clone();
    visit(&mut scratch, &mut |key, english| {
        out.push((key.to_owned(), english.to_owned()));
        None
    });
    out
}

/// Walk every translatable string of a manifest, handing each to `f` as
/// (key, current text) and taking a replacement back.
///
/// One walk for both jobs on purpose: a translator's key list and the
/// lookup that uses it cannot drift apart if they are the same code.
fn visit(manifest: &mut ExtensionManifest, f: &mut impl FnMut(&str, &str) -> Option<String>) {
    replace(&mut manifest.summary, "summary", f);
    for control in &mut manifest.pane {
        let path = format!("pane.{}", control_id(control));
        visit_control(control, &path, f);
    }
    // Tray entries have no key or id of their own, so they are named
    // after their English label. Reordering the menu therefore keeps
    // every translation; rewording an entry asks for a new one, which
    // is the right way round.
    for item in &mut manifest.tray_items {
        let key = format!("tray.{}", slug(&item.label));
        replace(&mut item.label, &key, f);
    }
    for list in &mut manifest.tray_lists {
        let path = format!("tray_list.{}", slug(&list.label));
        replace(&mut list.label, &format!("{path}.label"), f);
        replace(&mut list.empty_label, &format!("{path}.empty"), f);
        for action in &mut list.actions {
            let key = format!("{path}.action.{}", action.command.trim());
            replace(&mut action.label, &key, f);
        }
        for action in &mut list.bulk {
            let key = format!("{path}.bulk.{}", action.command.trim());
            replace(&mut action.label, &key, f);
        }
    }
}

fn visit_control(
    control: &mut PaneControl,
    path: &str,
    f: &mut impl FnMut(&str, &str) -> Option<String>,
) {
    replace(&mut control.label, &format!("{path}.label"), f);
    replace(&mut control.help, &format!("{path}.help"), f);
    replace(&mut control.add_label, &format!("{path}.add"), f);
    for option in &mut control.options {
        visit_option(option, path, f);
    }
    for action in &mut control.actions {
        let key = format!("{path}.action.{}", action.command.trim());
        replace(&mut action.label, &key, f);
    }
    // One level deep: `validate` refuses a records group inside a
    // records group, so this cannot recurse further.
    for field in &mut control.fields {
        let nested = format!("{path}.field.{}", control_id(field));
        visit_control(field, &nested, f);
    }
}

/// An option's label, which for a bare option *is* its value.
///
/// Translating one therefore has to grow it into the described form,
/// because the value has to stay behind unchanged: it is what lands in
/// the plug-in's config file.
fn visit_option(
    option: &mut PaneOption,
    path: &str,
    f: &mut impl FnMut(&str, &str) -> Option<String>,
) {
    let value = option.value().to_owned();
    let current = option.label().to_owned();
    let key = format!("{path}.option.{value}");
    let translated = if current.trim().is_empty() {
        None
    } else {
        f(&key, &current)
    };
    match option {
        PaneOption::Described { label, detail, .. } => {
            if let Some(text) = translated {
                *label = text;
            }
            replace(detail, &format!("{key}.detail"), f);
        }
        PaneOption::Value(_) => {
            if let Some(text) = translated {
                *option = PaneOption::Described {
                    value,
                    label: text,
                    detail: String::new(),
                    link: String::new(),
                };
            }
        }
    }
}

/// A string a plug-in did not write is not one a translator can be
/// offered, so an empty slot is skipped rather than keyed.
fn replace(slot: &mut String, key: &str, f: &mut impl FnMut(&str, &str) -> Option<String>) {
    if slot.trim().is_empty() {
        return;
    }
    if let Some(text) = f(key, slot) {
        *slot = text;
    }
}

/// What a control is called in a key: the config key it binds to, the
/// command it runs when it binds to nothing, and failing both — a
/// section, which does neither — a slug of its English label.
fn control_id(control: &PaneControl) -> String {
    for candidate in [control.key.trim(), control.command.trim()] {
        if !candidate.is_empty() {
            return candidate.to_owned();
        }
    }
    let slug = slug(&control.label);
    if slug.is_empty() {
        "unnamed".to_owned()
    } else {
        slug
    }
}

fn slug(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            out.extend(ch.to_lowercase());
        } else if !out.is_empty() && !out.ends_with('_') {
            out.push('_');
        }
        if out.len() >= SLUG_MAX {
            break;
        }
    }
    out.trim_end_matches('_').to_owned()
}

#[cfg(test)]
mod tests;
