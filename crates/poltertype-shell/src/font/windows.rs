//! The Windows UI font since Vista, present on every supported version.

#[must_use]
pub fn ui_font_family() -> Option<String> {
    Some("Segoe UI".to_owned())
}
