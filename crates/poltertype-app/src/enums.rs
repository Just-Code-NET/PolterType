//! Event-loop message enums.

use poltertype_core::engine::SwitcherEvent;
use tray_icon::menu::MenuId;

#[derive(Debug, Clone)]
pub(crate) enum UserEvent {
    Menu(MenuId),
    Hotkey(u32),
    Engine(SwitcherEvent),
}
