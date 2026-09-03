//! The popup thread's own loop: pumps the Wayland queue and serves
//! commands from the channel, parked on it while nothing is shown.

use std::io::ErrorKind;
use std::thread;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, TryRecvError};
use tracing::warn;
use wayland_client::backend::WaylandError;
use wayland_client::globals::GlobalList;
use wayland_client::{DispatchError, EventQueue, QueueHandle};

use super::enums::Cmd;
use super::state::WlState;
use crate::enums::PopupUiEvent;

/// Tick period while a surface is mapped.
const TICK: Duration = Duration::from_millis(16);

pub(super) fn run(
    globals: GlobalList,
    mut event_queue: EventQueue<WlState>,
    cmd_rx: Receiver<Cmd>,
    events: Sender<PopupUiEvent>,
) {
    let qh = event_queue.handle();
    let mut state = match WlState::new(&globals, &qh, events) {
        Ok(state) => state,
        Err(e) => {
            warn!(err = %e, "wayland popup thread failed to bind globals");
            return;
        }
    };

    loop {
        if state.view.is_some() {
            loop {
                match cmd_rx.try_recv() {
                    Ok(cmd) => serve(&mut state, &mut event_queue, &qh, cmd),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return,
                }
            }
            if let Err(e) = pump(&mut event_queue, &mut state) {
                warn!(err = %e, "wayland popup thread lost its connection");
                return;
            }
            state.check_deadline();
            thread::sleep(TICK);
        } else {
            // Push out any pending destroy before parking, then block
            // on the channel — zero CPU while hidden.
            if let Err(e) = pump(&mut event_queue, &mut state) {
                warn!(err = %e, "wayland popup thread lost its connection");
                return;
            }
            match cmd_rx.recv() {
                Ok(cmd) => serve(&mut state, &mut event_queue, &qh, cmd),
                Err(_) => return,
            }
        }
    }
}

/// Run one command, round-tripping the queue before a `Show`.
///
/// Placement needs the outputs' names, sizes and scales, which arrive
/// as *events*, not with the globals — and between popups the thread is
/// parked on the command channel reading nothing from the socket.
/// Without this round-trip the **first** popup of every session was
/// placed against an empty output list (no bounds, `output: None`), and
/// every later one was fine because the tick loop had pumped the queue:
/// the bug looked intermittent. Also picks up hotplugs that happened
/// while parked.
fn serve(
    state: &mut WlState,
    queue: &mut EventQueue<WlState>,
    qh: &QueueHandle<WlState>,
    cmd: Cmd,
) {
    if matches!(cmd, Cmd::Show(_)) {
        if let Err(e) = queue.roundtrip(state) {
            warn!(err = %e, "popup output refresh failed; placing with stale output info");
        }
    }
    state.handle_cmd(cmd, qh);
}

/// Non-blocking queue pump: flush requests, read whatever the socket
/// has (tolerating `WouldBlock`), dispatch to handlers.
fn pump(queue: &mut EventQueue<WlState>, state: &mut WlState) -> Result<(), DispatchError> {
    match queue.flush() {
        Ok(()) => {}
        Err(WaylandError::Io(e)) if e.kind() == ErrorKind::WouldBlock => {}
        Err(e) => return Err(DispatchError::Backend(e)),
    }
    if let Some(guard) = queue.prepare_read() {
        match guard.read() {
            Ok(_) => {}
            Err(WaylandError::Io(e)) if e.kind() == ErrorKind::WouldBlock => {}
            Err(e) => return Err(DispatchError::Backend(e)),
        }
    }
    queue.dispatch_pending(state).map(drop)
}
