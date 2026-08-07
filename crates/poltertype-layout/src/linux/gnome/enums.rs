//! What the probe does about desktops that ignore the schema.

/// Whether `try_init` may disqualify a session because the desktop
/// running it does not read `org.gnome.desktop.input-sources`.
pub(crate) enum UnreadSchema {
    /// Probing: stand down and let that desktop's own backend have the
    /// session.
    StandDown,
    /// Pinned through `POLTERTYPE_LAYOUT_BACKEND`: the user has seen
    /// gsettings switching work on their machine, and our list of
    /// desktops has not.
    Ignore,
}
