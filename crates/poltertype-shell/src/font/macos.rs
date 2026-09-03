//! Not the system UI font (`.AppleSystemUIFont` is not a family a font
//! database can look up), but the one macOS has shipped under a real
//! name for a decade.

#[must_use]
pub fn ui_font_family() -> Option<String> {
    Some("Helvetica Neue".to_owned())
}
