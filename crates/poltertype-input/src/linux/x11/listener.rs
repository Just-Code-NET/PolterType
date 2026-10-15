//! `X11Listener` — global key events via XInput2 raw events.

use super::events::*;
use crate::claims::KeyClaims;
use crate::{InputError, InputListener, KeyEvent};
use crossbeam_channel::Sender;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use tracing::info;

pub struct X11Listener {
    stop: Arc<AtomicBool>,
    /// The suggestion tooltip's claims, enforced with key grabs.
    claims: Option<Arc<KeyClaims>>,
}

impl X11Listener {
    pub fn new() -> Self {
        Self {
            stop: Arc::new(AtomicBool::new(false)),
            claims: None,
        }
    }

    pub(crate) fn with_claims(claims: Arc<KeyClaims>) -> Self {
        Self {
            claims: Some(claims),
            ..Self::new()
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
        let claims = self.claims.clone();
        thread::Builder::new()
            .name("poltertype-input-x11".into())
            .spawn(move || drain_events(conn, sink, stop, claims))
            .map_err(|e| InputError::Os(format!("spawn x11 thread: {e}")))?;
        if let Some(c) = self.claims.as_ref() {
            c.set_supported(true);
        }
        Ok(())
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(c) = self.claims.as_ref() {
            c.set_supported(false);
        }
    }

    fn backend_name(&self) -> &'static str {
        "linux-x11-xinput2"
    }
}
