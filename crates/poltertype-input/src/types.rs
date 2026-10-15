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

/// A set of [`ClaimKey`](crate::ClaimKey)s, small enough to live in one
/// atomic — the hook callbacks that read it may not take a lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClaimSet(u32);

impl ClaimSet {
    pub fn with(self, key: crate::ClaimKey) -> Self {
        Self(self.0 | key.bit())
    }

    pub fn contains(self, key: crate::ClaimKey) -> bool {
        self.0 & key.bit() != 0
    }

    /// The modifiers a claimed [`Digit`](crate::ClaimKey::Digit) must be
    /// pressed with — the accept chord's. Every other key is claimed
    /// only with none held.
    pub fn with_digit_modifiers(self, m: crate::Modifiers) -> Self {
        let bits = u32::from(m.control)
            | u32::from(m.shift) << 1
            | u32::from(m.alt) << 2
            | u32::from(m.meta) << 3;
        Self((self.0 & 0x00FF_FFFF) | bits << 24)
    }

    pub fn digit_modifiers(self) -> crate::Modifiers {
        let bit = |n: u32| self.0 >> (24 + n) & 1 != 0;
        crate::Modifiers {
            control: bit(0),
            shift: bit(1),
            alt: bit(2),
            meta: bit(3),
            ..crate::Modifiers::NONE
        }
    }

    pub fn is_empty(self) -> bool {
        self.0 & 0x00FF_FFFF == 0
    }

    pub fn keys(self) -> impl Iterator<Item = crate::ClaimKey> {
        crate::ClaimKey::all().filter(move |k| self.contains(*k))
    }

    pub(crate) fn bits(self) -> u32 {
        self.0
    }

    pub(crate) fn from_bits(bits: u32) -> Self {
        Self(bits)
    }
}
