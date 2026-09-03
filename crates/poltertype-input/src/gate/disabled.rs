//! The [`KeyGate`](crate::KeyGate) backend for a target with none of
//! the real ones.
//!
//! Never actually constructed: [`create_key_gate`](crate::create_key_gate)
//! on such a target returns [`KeyGate::disabled`](crate::KeyGate::disabled),
//! whose backend stays `None`. A type still has to exist for `KeyGate`'s
//! field to name.

pub(crate) struct DisabledGate;

impl DisabledGate {
    pub(crate) fn available(&self) -> bool {
        false
    }

    pub(crate) fn hold(&self) -> bool {
        false
    }

    pub(crate) fn release(&self) {}
}
