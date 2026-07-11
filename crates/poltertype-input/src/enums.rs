//! Input subsystem errors.

use crate::*;
pub use focus::{FocusTracker, NoopFocusTracker, create_focus_tracker};
pub use poltertype_types::{KeyDirection, KeyEvent, Modifiers};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum InputError {
    #[error("the active platform does not support a global keyboard listener: {0}")]
    Unsupported(String),
    #[error("OS error while installing keyboard hook: {0}")]
    Os(String),
    #[error("listener already started")]
    AlreadyStarted,
}
