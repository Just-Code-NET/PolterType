//! Device handles shared by the reader loop.

use super::*;
use crate::{
    EmittedKey, InputError, InputListener, KeyDirection, KeyEmitter, KeyEvent, Modifiers, ReplayKey,
};
use crossbeam_channel::Sender;
use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, Device, EventType, InputEvent, KeyCode};
use poltertype_types::SC_POINTER_BUTTON;
use std::collections::HashSet;
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use tracing::{debug, info, trace, warn};

/// The part of a device the key gate reasons about, separated from the
/// `evdev` handle so the gate's logic can be driven by a fake in tests
/// — grabbing real keyboards is not something a unit test can do.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct GateState {
    /// Our own uinput emitter, echoing back. The gate must leave it
    /// alone — grabbing it would hold back our own corrections.
    pub(crate) is_ours: bool,
    /// Advertises `KEY_A`. Mice and touchpads are opened too (a click
    /// moves the caret, which the engine must know about), but they
    /// cannot deliver the keystrokes a correction races, and every
    /// device the gate takes costs a slow `EVIOCGRAB(0)` to give back.
    pub(crate) is_keyboard: bool,
    /// When this device last produced an event. The gate only holds
    /// keyboards actually in use: on a typical machine that is one
    /// device out of a dozen, and the release cost is per device.
    pub(crate) last_event: Option<Instant>,
    /// The gate currently holds this device exclusively.
    pub(crate) grabbed: bool,
    /// Hold generation this device was last tried in. A device an
    /// input remapper already owns answers `EBUSY` every time, and
    /// retrying it on every poll would spend the read loop's budget on
    /// failing ioctls — so each device is attempted at most once per
    /// hold.
    pub(crate) tried_epoch: u64,
}

/// One opened keyboard, paired with its `/dev/input/event*` path so the
/// rescan loop can tell which devices it has already taken.
pub(crate) struct OpenDevice {
    pub(crate) path: PathBuf,
    pub(crate) dev: Device,
    pub(crate) gate: GateState,
}
