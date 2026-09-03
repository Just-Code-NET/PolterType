//! The X11 thread's loop: park on the command channel while hidden,
//! tick at ~16 ms via `poll_for_event` while a window is mapped.

use std::thread;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, TryRecvError};
use tracing::warn;
use x11rb::rust_connection::RustConnection;

use crate::enums::PopupUiEvent;
use crate::linux::x11::enums::Cmd;

use super::x11_state::X11State;

/// Tick period while the window is mapped.
const TICK: Duration = Duration::from_millis(16);

pub(crate) fn run(
    conn: RustConnection,
    screen_num: usize,
    cmd_rx: Receiver<Cmd>,
    events: Sender<PopupUiEvent>,
) {
    let mut state = match X11State::new(conn, screen_num, events) {
        Some(state) => state,
        None => {
            warn!("x11 popup thread failed to initialise");
            return;
        }
    };

    loop {
        if state.win.is_some() {
            loop {
                match cmd_rx.try_recv() {
                    Ok(cmd) => state.handle_cmd(cmd),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return,
                }
            }
            if !state.pump_events() {
                return;
            }
            state.check_deadline();
            thread::sleep(TICK);
        } else {
            match cmd_rx.recv() {
                Ok(cmd) => state.handle_cmd(cmd),
                Err(_) => return,
            }
        }
    }
}

impl X11State {
    fn handle_cmd(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Show(model) => {
                if let Err(e) = self.show(model) {
                    warn!(err = %e, "x11 popup show failed");
                    self.destroy_window();
                }
            }
            Cmd::Hide => self.destroy_window(),
        }
    }
}
