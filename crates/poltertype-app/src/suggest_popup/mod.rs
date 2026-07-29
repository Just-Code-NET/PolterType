//! Glue between the engine's suggestion events and the tooltip
//! backend: model building (`show.rs`) and anchor resolution
//! (`anchor.rs`).

mod anchor;
mod consts;
mod show;

pub(crate) use show::show_suggestion_popup;

#[cfg(test)]
mod tests;
