//! The do-nothing backend: platforms with no overlay path yet
//! (GNOME/KDE Wayland, macOS, Windows). The feature silently
//! degrades to the keyboard-accept flow driven by the engine.

use crate::traits::SuggestionPopup;
use crate::types::PopupModel;

pub struct NoopPopup;

impl SuggestionPopup for NoopPopup {
    fn show(&self, _model: PopupModel) {}

    fn hide(&self) {}

    fn backend_name(&self) -> &'static str {
        "noop"
    }
}
