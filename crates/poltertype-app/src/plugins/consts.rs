//! Constants other plug-in modules name.

/// The command id a plug-in may declare to be told "wind up now".
///
/// Reserved rather than invented per plug-in: the supervisor has to know
/// the name without being configured, and a plug-in that does not declare
/// it is simply not asked.
///
/// This is the graceful stop on **every** platform, because the per-OS
/// mechanisms are not: SIGTERM still needs the plug-in to install a
/// handler, and Windows' console control event was measured here and
/// refused — see `docs/DECISIONS.md`.
pub const STOP_COMMAND: &str = "stop";

/// The one argument a per-row command may have substituted.
pub const ROW_ID_PLACEHOLDER: &str = "{id}";
