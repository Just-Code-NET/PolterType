//! Sentinel scancodes shared across crates.

/// Pseudo-scancode the listeners report for a pointer-button press
/// (mouse click, touchpad tap); real SC Set-1 scancodes are < 0x200,
/// so it cannot collide. A click usually moves the caret, silently
/// invalidating the word buffer, so the engine reads this as "abandon
/// the current word and start fresh".
pub const SC_POINTER_BUTTON: u32 = 0xF000_0001;
