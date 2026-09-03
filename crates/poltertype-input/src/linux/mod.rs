//! Linux global keyboard listener + emitter, two backends picked by
//! session type:
//!
//! 1. **X11** — `XInput2` raw events, `XTest` for emitting. No special
//!    permissions: any client that can open the display can select raw
//!    events.
//! 2. **Wayland** — `evdev` for listening, `uinput` for emitting.
//!    Wayland has no global keyboard-snooping protocol by design, so
//!    reading `/dev/input/event*` is the realistic path. Needs the
//!    `input` group and a udev rule, both set up by
//!    `scripts/setup-linux.sh`; without them the listener returns
//!    `InputError::Os` and the tray shows an onboarding banner.
//!
//! There is no third backend and there will be no AT-SPI one:
//! `at-spi2-registryd` has no keyboard of its own — on Wayland it
//! relays only what the compositor hands it, and only mutter does
//! (measured on wlroots: `RegisterKeystrokeListener` returns false and
//! no events arrive even with injected keys). See `DECISIONS.md`,
//! 2026-08-01.

#![allow(unused_imports, dead_code)] // Linux-only code; Windows doesn't compile this.

pub(crate) mod access;
pub mod portal;
pub mod wayland;
pub mod x11;

pub(crate) mod factory;
mod session;

pub(crate) use factory::{create_emitter, create_key_gate, create_listener};
pub(crate) use session::{SessionKind, session_kind};
