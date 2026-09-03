//! Platforms with no glyphs and no update-permission caveat: the
//! portable names in `config.toml` are what the user sees.

pub fn key_glyph(token: &str) -> Option<&'static str> {
    let _ = token;
    None
}

pub fn update_permission_note() -> Option<&'static str> {
    None
}
