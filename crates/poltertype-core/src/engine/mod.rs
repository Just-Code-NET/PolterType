//! `SwitcherEngine` — the state machine that turns key events into
//! layout-switch actions.
//!
//! Lives on a worker thread, receives [`KeyEvent`]s from the OS hook and
//! emits [`SwitcherEvent`]s back to the application. The actual switch
//! and key replay happen in `poltertype-app`, through the emitter and
//! switcher passed in.

pub mod buffer;
pub mod decision;

mod consts;
mod enums;
mod heuristics;
mod switcher;
mod types;

pub use buffer::{WordBoundary, WordBuffer};
pub use decision::DecisionPolicy;
pub use enums::{DictionaryAddOrigin, EngineCommand, SwitcherEvent};
pub use switcher::{EngineDeps, SwitcherEngine};
pub use types::{
    AcceptModifiers, Binding, Chord, KeystreamHotkeys, ModChord, ModRole, ModSet, SuggestionAction,
    SuggestionEntry,
};

#[cfg(test)]
mod tests;
