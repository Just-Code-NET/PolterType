//! `EvdevListener` — reads raw key events from /dev/input.

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

pub struct EvdevListener {
    stop: Arc<AtomicBool>,
    /// Shared with whoever asks for correction-time holds. Only this
    /// listener's device thread ever touches the devices themselves.
    gate: Arc<EvdevGate>,
}

impl EvdevListener {
    pub fn new() -> Self {
        Self::with_gate(Arc::new(EvdevGate::new()))
    }

    pub(crate) fn with_gate(gate: Arc<EvdevGate>) -> Self {
        Self {
            stop: Arc::new(AtomicBool::new(false)),
            gate,
        }
    }
}

impl InputListener for EvdevListener {
    fn start(&mut self, sink: Sender<KeyEvent>) -> Result<(), InputError> {
        let devices = open_keyboard_devices();
        if devices.is_empty() {
            return Err(InputError::Os(
                "no readable keyboard devices in /dev/input/* — \
                 run scripts/setup-linux.sh to grant access"
                    .into(),
            ));
        }
        info!(count = devices.len(), "opened evdev keyboard devices");

        // Decide once, now that the emitter's device exists, whether
        // holding keystrokes back during corrections is safe here.
        self.gate.probe_availability();

        let stop = Arc::clone(&self.stop);
        let gate = Arc::clone(&self.gate);
        thread::Builder::new()
            .name("poltertype-input-evdev".into())
            .spawn(move || drain_devices(devices, sink, stop, gate))
            .map_err(|e| InputError::Os(format!("spawn evdev thread: {e}")))?;
        Ok(())
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    fn backend_name(&self) -> &'static str {
        "linux-wayland-evdev"
    }
}
