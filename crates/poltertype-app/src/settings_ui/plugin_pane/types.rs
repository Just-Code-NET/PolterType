//! Plain data addressed by, or held for, a box on the pane.

use poltertype_core::plugins::SettingValue;

/// Which box on the pane is being talked about.
///
/// A control index alone is not enough: the fields inside a repeating
/// group's cards are controls too, they can carry a command of their
/// own, and each *card* holds its own half-typed text. So a box is
/// named by all three — control, declared field, card.
///
/// The command behind a field is asked once for the whole group rather
/// than once per card: which conversations exist is a question about the
/// chat client, not about the row. That answer is filed under
/// [`Self::asked`] — this slot with the card forgotten.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Slot {
    pub control: usize,
    /// Position in the control's `fields`. `None` — the control itself.
    pub field: Option<usize>,
    /// Which card of a repeating group. `None` — not in one.
    pub row: Option<usize>,
}

impl Slot {
    /// One of the plug-in's own controls.
    pub const fn control(control: usize) -> Self {
        Self {
            control,
            field: None,
            row: None,
        }
    }

    /// One field of one card.
    pub const fn field(control: usize, row: usize, field: usize) -> Self {
        Self {
            control,
            field: Some(field),
            row: Some(row),
        }
    }

    /// The same box with the card forgotten — what a command's answer is
    /// filed under, since one answer serves every card.
    pub const fn asked(self) -> Self {
        Self { row: None, ..self }
    }
}

/// One row of a list control.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListRow {
    /// What goes into the config array when the box is ticked.
    pub id: String,
    /// What the user reads.
    pub label: String,
    /// A line under it — where a row says what was measured about it.
    pub detail: String,
}

/// One row of a repeating group: its declared fields, and what the file
/// holds for each. `None` for a field the row omits — the plug-in's own
/// default applies and this pane does not know it.
pub type RecordRow = std::collections::HashMap<String, Option<SettingValue>>;
