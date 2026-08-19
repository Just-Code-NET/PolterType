//! Key replay / emission data carried across the traits.

pub use poltertype_types::KeyDirection;

/// A scancode + shift state pair, to be replayed against whatever
/// layout the OS is currently in. Used by the Linux corrector to
/// avoid the Unicode-input compose dance that breaks in terminals
/// and Wayland-native apps.
#[derive(Debug, Clone, Copy)]
pub struct ReplayKey {
    pub scancode: u32,
    pub shift: bool,
}

/// One synthetic keystroke an emitter actually put on the wire.
///
/// Where injected events echo back indistinguishable from real typing
/// — Linux/uinput behind a remapper — the engine collects these via
/// [`KeyEmitter::take_emitted`] and match-and-consumes the echoes,
/// rather than suppressing everything for a fixed window, which eats
/// the first characters typed after a correction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmittedKey {
    pub scancode: u32,
    pub direction: KeyDirection,
}
