//! Environment override knob.

/// Env var an operator (or a test) sets to pin the data dir
/// explicitly. Highest-priority lookup in [`resolve`].
pub const ENV_OVERRIDE: &str = "POLTERTYPE_DATA_DIR";
