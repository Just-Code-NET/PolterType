//! The focus tracker used when compiled for a target with no real one.

use std::sync::Arc;

use super::noop::NoopFocusTracker;
use super::traits::FocusTracker;

pub(crate) fn create_unsupported_focus_tracker(_caret_anchor: bool) -> Arc<dyn FocusTracker> {
    Arc::new(NoopFocusTracker)
}
