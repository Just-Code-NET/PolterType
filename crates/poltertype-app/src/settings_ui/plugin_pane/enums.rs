//! What a command-backed box is showing, and what box is being typed
//! into right now.

/// What a control that has to *ask the plug-in* is showing right now.
///
/// Shared by the report, which shows the text, and the list, which
/// parses rows out of it: one cache, one place that knows a command has
/// been asked for. Three states and not two — a pane that shows an
/// empty box for both "waiting" and "got nothing" looks broken while it
/// is working.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandOutput {
    Loading,
    /// It answered. May legitimately be empty text.
    Ready(String),
    /// It could not be asked, or it failed.
    Failed(String),
}

/// The box the cursor is in.
///
/// Passed to [`super::PluginPane::flush_edits`] so that settling
/// everything else does not settle what somebody is halfway through
/// typing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Typing {
    Control(usize),
    Record {
        control: usize,
        row: usize,
        field: String,
    },
}
