//! Module-wide statics.

use std::sync::OnceLock;

use crossbeam_channel::Sender;

use crate::enums::PopupUiEvent;

/// The app halves of every `PopupUiEvent` flow back through this.
/// Set once at construction; the panel outlives it.
pub(super) static EVENTS: OnceLock<Sender<PopupUiEvent>> = OnceLock::new();
