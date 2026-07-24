//! `SwitcherEngine` — the state machine that turns key events into
//! layout-switch actions.
//!
//! Lives on a worker thread; receives [`KeyEvent`]s from the OS hook
//! and emits [`SwitcherEvent`]s back to the application (so the tray
//! / UI / audio can react). Effecting the actual switch + key replay
//! happens in `poltertype-app` via the [`poltertype_input::KeyEmitter`] +
//! [`poltertype_layout::LayoutSwitcher`] passed in.

pub mod buffer;
pub mod decision;

mod consts;
mod enums;
mod heuristics;
mod switcher;
mod types;

pub use buffer::{WordBoundary, WordBuffer};
pub use decision::DecisionPolicy;
pub use enums::{EngineCommand, SwitcherEvent};
pub use switcher::SwitcherEngine;
pub use types::{AcceptModifiers, Chord, KeystreamHotkeys, SuggestionAction, SuggestionEntry};

#[cfg(test)]
mod tests;
