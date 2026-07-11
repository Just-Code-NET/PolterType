//! Sentinel scancodes shared across crates.

/// Pseudo-scancode the listeners report for a pointer-button press
/// (mouse click, touchpad tap). Real SC Set-1 scancodes are < 0x200,
/// so this can never collide. A click usually moves the caret or the
/// focus, which silently invalidates the engine's word buffer — the
/// engine treats this scancode as "abandon the current word and start
/// fresh at the new caret position".
pub const SC_POINTER_BUTTON: u32 = 0xF000_0001;
