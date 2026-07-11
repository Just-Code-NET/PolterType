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
}

impl EvdevListener {
    pub fn new() -> Self {
        Self {
            stop: Arc::new(AtomicBool::new(false)),
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

        let stop = Arc::clone(&self.stop);
        thread::Builder::new()
            .name("poltertype-input-evdev".into())
            .spawn(move || drain_devices(devices, sink, stop))
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
