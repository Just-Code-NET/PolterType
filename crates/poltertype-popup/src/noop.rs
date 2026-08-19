//! The do-nothing backend, reached when every probe in
//! [`crate::create_popup`] fails. The feature silently degrades to the
//! keyboard-accept flow driven by the engine.

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
