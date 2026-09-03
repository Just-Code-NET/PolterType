//! The do-nothing backend, reached when every probe in
//! [`crate::create_popup`] fails. The feature silently degrades to the
//! keyboard-accept flow driven by the engine.

use crossbeam_channel::Sender;

use crate::enums::PopupUiEvent;
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

/// The factory's answer on a target that is none of Linux, Windows or
/// macOS. Unreachable on every OS this crate ships for, so it is dead
/// code there — kept because it is what makes an unlisted target build
/// instead of failing to link.
#[allow(dead_code)]
pub(crate) fn create_for_platform(_events: Sender<PopupUiEvent>) -> Box<dyn SuggestionPopup> {
    Box::new(NoopPopup)
}
