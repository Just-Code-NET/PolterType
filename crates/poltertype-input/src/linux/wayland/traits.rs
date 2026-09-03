//! What the key gate needs of a device.

use super::types::{GateState, OpenDevice};

/// What the key gate needs of a device: the two grab syscalls and the
/// bookkeeping around them. `OpenDevice` is the real implementation;
/// tests supply a fake that can fail, be slow, or report itself as a
/// mouse, so the gate's decisions are testable without a keyboard.
pub(crate) trait GateDevice {
    fn grab(&mut self) -> std::io::Result<()>;
    fn ungrab(&mut self) -> std::io::Result<()>;
    fn state(&self) -> &GateState;
    fn state_mut(&mut self) -> &mut GateState;
    fn label(&self) -> String;
}

impl GateDevice for OpenDevice {
    fn grab(&mut self) -> std::io::Result<()> {
        self.dev.grab()
    }

    fn ungrab(&mut self) -> std::io::Result<()> {
        self.dev.ungrab()
    }

    fn state(&self) -> &GateState {
        &self.gate
    }

    fn state_mut(&mut self) -> &mut GateState {
        &mut self.gate
    }

    fn label(&self) -> String {
        self.path.display().to_string()
    }
}
