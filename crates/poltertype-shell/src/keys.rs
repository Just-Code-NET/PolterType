//! How the platform writes the names of keys.
//!
//! `config.toml` always stores portable names (`Ctrl`, `Alt`,
//! `Meta`, `Backspace`), and the parser only ever sees those. This is
//! purely what the Settings window *shows*: a Mac keyboard has ⌃ ⌥ ⇧ ⌘
//! printed on it and nothing that says "Meta", so a hotkey rendered in
//! portable names is a small puzzle for the user to solve. Windows and
//! Linux keyboards print the words, so there the words are right.

/// The glyph a platform prints on this key, if it prints one.
///
/// `None` means "the portable name is what the user sees", which is
/// the answer everywhere except macOS. Matching is case-insensitive
/// because the token comes from `config.toml`, where the user may
/// have typed anything the parser accepts.
pub fn key_glyph(token: &str) -> Option<&'static str> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = token;
        None
    }
    #[cfg(target_os = "macos")]
    {
        Some(match token.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => "⌃",
            "alt" | "option" => "⌥",
            "shift" => "⇧",
            "meta" | "cmd" | "command" | "super" | "win" => "⌘",
            "backspace" => "⌫",
            "enter" | "return" => "↩",
            "tab" => "⇥",
            "esc" | "escape" => "⎋",
            "space" => "Space",
            "up" => "↑",
            "down" => "↓",
            "left" => "←",
            "right" => "→",
            _ => return None,
        })
    }
}

/// The key's name, annotated with its glyph where the platform has
/// one: `"Ctrl"` → `"Ctrl (⌃)"` on macOS, `"Ctrl"` elsewhere.
///
/// For prose that has to stay readable as *instructions* — the
/// setting is still spelled `Ctrl` in the file, so showing only the
/// glyph would leave the user guessing what to type.
pub fn key_name_with_glyph(name: &str) -> String {
    match key_glyph(name) {
        Some(glyph) if glyph != name => format!("{name} ({glyph})"),
        _ => name.to_owned(),
    }
}
