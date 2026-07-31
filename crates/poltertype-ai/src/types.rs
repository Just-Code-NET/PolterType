//! Configuration for AI plug-ins.
//!
//! The schema itself lives in `poltertype-types` so that
//! `poltertype-core` can parse it without depending on this optional
//! crate; re-exported here because this is where callers look for it.

pub use poltertype_types::AiPluginConfig;
