//! `X11Listener` — global key events via XInput2 raw events.

use super::events::*;
use crate::{InputError, InputListener, KeyEvent};
use crossbeam_channel::Sender;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use tracing::info;

pub struct X11Listener {
    stop: Arc<AtomicBool>,
}

impl X11Listener {
    pub fn new() -> Self {
        Self {
            stop: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Default for X11Listener {
    fn default() -> Self {
        Self::new()
    }
}

impl InputListener for X11Listener {
    fn start(&mut self, sink: Sender<KeyEvent>) -> Result<(), InputError> {
        let conn = connect_and_select()?;
        info!("x11 listener started (no input-group membership required)");

        let stop = Arc::clone(&self.stop);
        thread::Builder::new()
            .name("poltertype-input-x11".into())
            .spawn(move || drain_events(conn, sink, stop))
            .map_err(|e| InputError::Os(format!("spawn x11 thread: {e}")))?;
        Ok(())
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    fn backend_name(&self) -> &'static str {
        "linux-x11-xinput2"
    }
}
