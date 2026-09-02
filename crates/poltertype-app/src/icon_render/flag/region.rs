//! Which country a layout belongs to.

use poltertype_types::LayoutId;

/// The region subtag of a layout id — `uk-UA` → `UA`, `kk-Cyrl-KZ` →
/// `KZ` — uppercased for the table.
///
/// The *region*, never the language: a flag stands for a country, and
/// `en-GB` and `en-US` are the same language behind two of them.
/// A tag with no region (`ar`), a UN area code (`es-419`) and the
/// opaque ids Windows and macOS fall back to all answer `None`, which
/// is what sends the icon back to its letters.
pub(crate) fn region_of(id: &LayoutId) -> Option<String> {
    let mut parts = id.as_str().split('-');
    parts.next()?;
    parts
        .rfind(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_alphabetic()))
        .map(str::to_ascii_uppercase)
}
