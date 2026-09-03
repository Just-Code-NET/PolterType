//! Plain data plug-ins hand back across the process boundary.

/// A service that has exited, and the shortest true answer to "why".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Departed {
    pub id: String,
    /// Exit status, plus the last thing the plug-in said if it said
    /// anything. Already one line, already bounded — it goes in a
    /// notification.
    pub why: String,
}

/// One entry of a plug-in's runtime menu, as the plug-in printed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuRow {
    /// Handed back to the plug-in when an action on this row is chosen.
    pub id: String,
    /// The line the user reads in the menu.
    pub label: String,
    /// Lines shown under it, disabled. This is where a row says what it
    /// actually holds — who wrote, what the reply would be — without a
    /// window having to be opened to find out.
    pub details: Vec<String>,
}
