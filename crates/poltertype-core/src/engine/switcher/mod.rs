//! `SwitcherEngine` itself — state, run loop, and the correction
//! machinery. Pure helpers live in [`super::heuristics`], plain
//! data types in [`super::types`].
//!
//! One `impl` block per concern, one file per `impl` block:
//!
//! | File | Concern |
//! |---|---|
//! | [`engine`] | the struct, its fields, construction |
//! | [`types`] | [`EngineDeps`], the struct's construction parameters |
//! | [`run_loop`] | channel multiplexing, command + key dispatch |
//! | [`echo`] | consuming echoes of our own injected keystrokes |
//! | [`decide`] | per-word decision: filters + detector pipeline |
//! | [`correction`] | emitting the correction; absorbing raced input |
//! | [`commands`] | keystream hotkey chords, smart-command dispatch |
//! | [`suggestions`] | spelling-suggestion offers, accepts, dismissal |

mod commands;
mod correction;
mod decide;
mod echo;
mod engine;
mod run_loop;
mod suggestions;
mod types;

pub use engine::SwitcherEngine;
pub use types::EngineDeps;
