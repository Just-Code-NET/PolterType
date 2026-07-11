//! The focus-tracking extension point.

/// Best-effort identifier of the currently-focused application.
pub trait FocusTracker: Send + Sync {
    /// File-name of the focused process's executable, e.g.
    /// `"Code.exe"` / `"alacritty"`. Returns `None` if no foreground
    /// window exists, the OS denies the query, or this platform's
    /// implementation is a stub.
    fn focused_exe(&self) -> Option<String>;

    fn backend_name(&self) -> &'static str;
}
