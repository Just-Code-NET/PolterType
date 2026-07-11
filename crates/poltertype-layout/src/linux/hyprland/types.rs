//! Parsed shapes of `hyprctl devices` output.

/// One keyboard block from `hyprctl devices`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KeyboardBlock {
    /// Device name as Hyprland prints it (already normalised by the
    /// compositor: lowercase, spaces → dashes).
    pub(crate) name: String,
    /// The `active keymap:` line, if the block had one.
    pub(crate) keymap: Option<String>,
    /// Whether the block carried `main: yes`.
    pub(crate) main: bool,
}
