//! Event-loop message enums.

use poltertype_core::engine::SwitcherEvent;
use poltertype_popup::PopupUiEvent;
use poltertype_update::PendingUpdate;
use tray_icon::menu::MenuId;

#[derive(Debug, Clone)]
pub(crate) enum UserEvent {
    Menu(MenuId),
    Hotkey(u32),
    Engine(SwitcherEvent),
    /// Suggestion-tooltip interaction (click / timeout).
    Popup(PopupUiEvent),
    Update(UpdateOutcome),
    /// `config.toml` has been re-read — because the Settings window
    /// closed, or because the watcher saw the file change under a
    /// running app. Carried through the event loop because the hotkey
    /// grabs live there and are not `Send`; whoever sends this has
    /// already reloaded the store.
    SettingsChanged,
    /// Time to re-ask every plug-in what state it is in, so the tray
    /// reflects a change made somewhere else — from the command line,
    /// or an authority that expired on its own.
    PluginState,
}

/// What the background update worker found. Reported to the event loop
/// so that the tray — which owns the menu — decides what to show; the
/// worker itself never touches UI.
#[derive(Debug, Clone)]
pub(crate) enum UpdateOutcome {
    /// A newer release was downloaded and its checksum verified. It is
    /// staged and will install on the next restart. `Box`ed because the
    /// other variants are a word or two, and without it this one would
    /// set the size of every event the loop copies around.
    Staged(Box<PendingUpdate>),
    UpToDate,
    /// The check didn't complete — no network, a proxy, GitHub having a
    /// bad day. The reason is logged by the worker and deliberately not
    /// carried here: nothing in the tray reacts to *why* it failed.
    Failed,
    /// Updates were switched off, and a staged artifact was discarded.
    Cleared,
}
