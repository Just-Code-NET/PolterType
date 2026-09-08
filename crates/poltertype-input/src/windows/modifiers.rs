//! The modifier set a key event carries, read from the OS inside the
//! low-level hook — and corrected for the one thing that read gets
//! wrong there: the event being delivered has not taken effect yet.

use poltertype_types::ModifierKey;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyState, VIRTUAL_KEY, VK_CAPITAL, VK_CONTROL, VK_LCONTROL, VK_LMENU,
    VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
};

use crate::{KeyDirection, Modifiers};

/// The modifier set *after* the event being delivered.
///
/// `GetAsyncKeyState` inside `WH_KEYBOARD_LL` describes the keyboard as
/// it was before this event: on the Ctrl release it still says "Ctrl
/// held", and with nothing typed afterwards that reading is the last
/// one the engine gets. It then believes the force-switch chord is
/// still down and waits for a release it has already been handed —
/// measured on Windows Server 2025 over RDP (2026-09-03, 2026-09-08):
/// every manual switch stalled until the next keystroke, which was the
/// user pressing the hotkey again. For a modifier the event is about,
/// the event itself is the truth; the other side of the same modifier
/// is read live, so releasing one Shift while the other is held keeps
/// Shift.
pub(super) fn modifiers_for_event(vk: u32, direction: KeyDirection) -> Modifiers {
    let snapshot = read_modifiers();
    let Some((key, other_side)) = modifier_key_of(vk) else {
        return snapshot;
    };
    let other_side_down = other_side.is_some_and(key_down);
    snapshot.after_transition(key, direction == KeyDirection::Press, other_side_down)
}

/// The modifier set as the OS reports it at the moment of the call.
///
/// Inside a low-level hook that moment is *before* the event being
/// delivered has taken effect: a Ctrl release still reads "Ctrl held".
/// [`modifiers_for_event`] is what corrects for that; this alone is
/// only right for keys the event is not about.
fn read_modifiers() -> Modifiers {
    // The low bit of `GetKeyState` is the toggle, not the held-ness —
    // the only thing that answers "is Caps Lock on" without guessing.
    // Safety: GetKeyState is a trivial Win32 call.
    let caps = unsafe { GetKeyState(VK_CAPITAL.0 as i32) } & 1 != 0;
    Modifiers {
        shift: key_down(VK_SHIFT),
        control: key_down(VK_CONTROL),
        alt: key_down(VK_MENU) || key_down(VK_LMENU) || key_down(VK_RMENU),
        meta: key_down(VK_LWIN) || key_down(VK_RWIN),
        caps,
    }
}

/// Is `vk` physically down right now, as the OS sees it.
fn key_down(vk: VIRTUAL_KEY) -> bool {
    // Safety: GetAsyncKeyState is a trivial Win32 call.
    unsafe { (GetAsyncKeyState(vk.0 as i32) as u16) & 0x8000 != 0 }
}

/// Which modifier a virtual key is, and the key on the other side of
/// the keyboard that holds the same modifier — `None` for the generic
/// codes injected input uses, which have no other side.
fn modifier_key_of(vk: u32) -> Option<(ModifierKey, Option<VIRTUAL_KEY>)> {
    let vk = VIRTUAL_KEY(u16::try_from(vk).ok()?);
    Some(match vk {
        VK_LSHIFT => (ModifierKey::Shift, Some(VK_RSHIFT)),
        VK_RSHIFT => (ModifierKey::Shift, Some(VK_LSHIFT)),
        VK_SHIFT => (ModifierKey::Shift, None),
        VK_LCONTROL => (ModifierKey::Control, Some(VK_RCONTROL)),
        VK_RCONTROL => (ModifierKey::Control, Some(VK_LCONTROL)),
        VK_CONTROL => (ModifierKey::Control, None),
        VK_LMENU => (ModifierKey::Alt, Some(VK_RMENU)),
        VK_RMENU => (ModifierKey::Alt, Some(VK_LMENU)),
        VK_MENU => (ModifierKey::Alt, None),
        VK_LWIN => (ModifierKey::Meta, Some(VK_RWIN)),
        VK_RWIN => (ModifierKey::Meta, Some(VK_LWIN)),
        _ => return None,
    })
}
