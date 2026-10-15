//! Glue between the engine's suggestion events and the tooltip
//! backend: model building (`show.rs`) and anchor resolution
//! (`anchor.rs`).

mod anchor;
mod consts;
mod show;
mod types;

pub(crate) use show::show_suggestion_popup;
pub(crate) use types::PopupOffer;

#[cfg(test)]
mod tests;
