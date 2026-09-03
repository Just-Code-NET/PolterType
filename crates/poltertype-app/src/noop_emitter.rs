//! Fallback key emitter for when no real backend is available, so a
//! missing permission degrades corrections to a no-op instead of a crash.

use anyhow::Result;
use tracing::debug;

pub(crate) struct NoopEmitter;

impl poltertype_input::KeyEmitter for NoopEmitter {
    fn send_backspaces(&self, n: usize) -> Result<(), poltertype_input::InputError> {
        debug!(n, "noop emitter: would send backspaces");
        Ok(())
    }
    fn send_text(&self, text: &str) -> Result<(), poltertype_input::InputError> {
        debug!(text, "noop emitter: would send text");
        Ok(())
    }
    fn backend_name(&self) -> &'static str {
        "noop"
    }
}

pub(crate) fn noop_emitter() -> Box<dyn poltertype_input::KeyEmitter> {
    Box::new(NoopEmitter)
}
