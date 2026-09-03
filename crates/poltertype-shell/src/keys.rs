//! How the platform writes the names of keys.
//!
//! `config.toml` always stores portable names and the parser only ever
//! sees those; this is purely what the Settings window *shows*. A Mac
//! keyboard has ⌃ ⌥ ⇧ ⌘ printed on it and nothing that says "Meta",
//! while Windows and Linux keyboards print the words.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(target_os = "macos"))]
mod other;

#[cfg(target_os = "macos")]
use macos as imp;
#[cfg(not(target_os = "macos"))]
use other as imp;

/// The glyph a platform prints on this key, if it prints one.
///
/// `None` means "the portable name is what the user sees", which is
/// the answer everywhere except macOS. Matching is case-insensitive
/// because the token comes from `config.toml`, where the user may
/// have typed anything the parser accepts.
pub use imp::key_glyph;

/// What an in-place self-update costs the user's permissions here, if
/// anything. `None` on the platforms where an update costs nothing.
///
/// macOS ties a privacy grant to the *code* it was given to. A
/// Developer ID signature keys it to the team identifier, and the grant
/// then survives the bundle being replaced; our builds are ad-hoc
/// signed (see `docs/CODE_SIGNING.md`), so TCC keys it to the code
/// directory hash instead, and every update makes a new hash. The
/// switch is still on in System Settings, the app is denied anyway,
/// and macOS suppresses its own prompt because a record exists —
/// which is why this has to be said *before* the update, not
/// discovered after it (issue #42).
pub use imp::update_permission_note;

/// The key's name, annotated with its glyph where the platform has
/// one: `"Ctrl"` → `"Ctrl (⌃)"` on macOS, `"Ctrl"` elsewhere.
///
/// For prose that has to stay readable as *instructions* — the setting
/// is still spelled `Ctrl` in the file.
pub fn key_name_with_glyph(name: &str) -> String {
    match key_glyph(name) {
        Some(glyph) if glyph != name => format!("{name} ({glyph})"),
        _ => name.to_owned(),
    }
}
