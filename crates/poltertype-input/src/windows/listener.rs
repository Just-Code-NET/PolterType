//! Windows global keyboard listener via `WH_KEYBOARD_LL`.
//!
//! The hook callback runs on the OS-owned hook thread; we pump it via a
//! dedicated thread that owns the message loop. The callback itself
//! does the absolute minimum: build a [`KeyEvent`] and try-send it
//! through a channel. Anything blocking would freeze the user's input
//! globally — see Microsoft's note on `LowLevelKeyboardProc` latency.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};

use std::sync::Arc;

use crossbeam_channel::Sender;
use tracing::{debug, error, info, warn};
use windows::Win32::Foundation::{HMODULE, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_LMENU, VK_LWIN, VK_MENU, VK_RMENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, MSG,
    PostThreadMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use super::consts::{EMITTER_MARKER, LLKHF_INJECTED_ANY};
use super::gate::WindowsGate;
use crate::{InputError, InputListener, KeyDirection, KeyEvent, Modifiers};

/// Sender shared with the C-callable hook procedure. There can only be
/// one global keyboard hook per process at a time. `parking_lot::RwLock`
/// gives us a non-poisoning shared cell without pulling in any global
/// allocator dance.
static EVENT_SINK: OnceLock<parking_lot::RwLock<Option<Sender<KeyEvent>>>> = OnceLock::new();

/// The key gate, shared with the C-callable hook procedure the same way
/// the sink is. `None` until a listener is started, and on a build where
/// the gate is switched off — the callback then never swallows anything.
static GATE: OnceLock<parking_lot::RwLock<Option<Arc<WindowsGate>>>> = OnceLock::new();

fn gate_slot() -> &'static parking_lot::RwLock<Option<Arc<WindowsGate>>> {
    GATE.get_or_init(|| parking_lot::RwLock::new(None))
}

fn sink_slot() -> &'static parking_lot::RwLock<Option<Sender<KeyEvent>>> {
    EVENT_SINK.get_or_init(|| parking_lot::RwLock::new(None))
}

pub struct WindowsListener {
    worker: Option<WorkerHandle>,
    gate: Option<Arc<WindowsGate>>,
}

struct WorkerHandle {
    join: JoinHandle<()>,
    thread_id: u32,
    stopping: std::sync::Arc<AtomicBool>,
}

impl WindowsListener {
    pub fn new() -> Self {
        Self {
            worker: None,
            gate: None,
        }
    }

    /// Wire the listener to the gate the engine holds, so the hook
    /// callback can consult it. Without this the callback swallows
    /// nothing, which is exactly the pre-0.8 behaviour.
    pub(crate) fn with_gate(gate: Arc<WindowsGate>) -> Self {
        Self {
            worker: None,
            gate: Some(gate),
        }
    }
}

impl InputListener for WindowsListener {
    fn start(&mut self, sink: Sender<KeyEvent>) -> Result<(), InputError> {
        if self.worker.is_some() {
            return Err(InputError::AlreadyStarted);
        }

        *sink_slot().write() = Some(sink);
        *gate_slot().write() = self.gate.clone();

        let stopping = std::sync::Arc::new(AtomicBool::new(false));
        let stopping_clone = stopping.clone();
        let (tid_tx, tid_rx) = crossbeam_channel::bounded::<u32>(1);

        let join = thread::Builder::new()
            .name("poltertype-input-windows-hook".into())
            .spawn(move || run_hook_thread(tid_tx, stopping_clone))
            .map_err(|e| InputError::Os(format!("spawn hook thread: {e}")))?;

        let thread_id = tid_rx.recv().map_err(|_| {
            InputError::Os("hook thread did not report its id (likely failed to install)".into())
        })?;

        info!(thread_id, "Windows LL keyboard hook installed");
        self.worker = Some(WorkerHandle {
            join,
            thread_id,
            stopping,
        });
        Ok(())
    }

    fn stop(&mut self) {
        let Some(worker) = self.worker.take() else {
            return;
        };
        worker.stopping.store(true, Ordering::SeqCst);
        // Wake the message loop so it can observe the flag and exit.
        // Safety: posting WM_QUIT to a known thread id is safe.
        unsafe {
            let _ = PostThreadMessageW(worker.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        if let Err(panic) = worker.join.join() {
            warn!(?panic, "hook thread panicked while shutting down");
        }
        if let Some(slot) = EVENT_SINK.get() {
            *slot.write() = None;
        }
        // Drop the gate reference too: a hook that is gone must not
        // leave a swallow decision reachable behind it.
        if let Some(slot) = GATE.get() {
            *slot.write() = None;
        }
        info!("Windows LL keyboard hook removed");
    }

    fn backend_name(&self) -> &'static str {
        "windows-ll-hook"
    }
}

impl Drop for WindowsListener {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run_hook_thread(tid_tx: Sender<u32>, _stopping: std::sync::Arc<AtomicBool>) {
    // Safety: GetCurrentThreadId is a trivial Win32 call.
    let tid = unsafe { windows::Win32::System::Threading::GetCurrentThreadId() };
    let _ = tid_tx.send(tid);

    // Safety: SetWindowsHookExW with WH_KEYBOARD_LL accepts NULL hMod
    // for an in-process global hook.
    let hook = unsafe {
        SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(low_level_keyboard_proc),
            HMODULE(std::ptr::null_mut()),
            0,
        )
    };
    let hook = match hook {
        Ok(h) => h,
        Err(e) => {
            error!(?e, "SetWindowsHookExW(WH_KEYBOARD_LL) failed");
            return;
        }
    };

    let mut msg = MSG::default();
    loop {
        // Safety: standard Win32 message pump. GetMessageW blocks the
        // thread until a message arrives or returns 0/-1 on quit/error.
        let r = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if r.0 == 0 || r.0 == -1 {
            break;
        }
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    // Safety: hook handle was returned by SetWindowsHookExW.
    unsafe {
        let _ = UnhookWindowsHookEx(hook);
    }
}

unsafe extern "system" fn low_level_keyboard_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code == HC_ACTION as i32 {
        // Safety: by Win32 contract, lparam points to a KBDLLHOOKSTRUCT.
        let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };

        let direction = match wparam.0 as u32 {
            WM_KEYDOWN | WM_SYSKEYDOWN => Some(KeyDirection::Press),
            WM_KEYUP | WM_SYSKEYUP => Some(KeyDirection::Release),
            _ => None,
        };

        if let Some(direction) = direction {
            // Ours, by the marker the emitter stamps into dwExtraInfo.
            // Distinct from "injected": another automation tool's
            // synthetic keys are injected too, and the gate holds those
            // back exactly like the user's.
            let ours = kb.dwExtraInfo == EMITTER_MARKER;
            let injected = ours || (kb.flags.0 & LLKHF_INJECTED_ANY) != 0;
            let event = KeyEvent {
                vk: kb.vkCode,
                scancode: kb.scanCode,
                direction,
                modifiers: read_modifiers(),
                injected,
                timestamp_ms: kb.time as u64,
            };

            if let Some(slot) = EVENT_SINK.get() {
                if let Some(sink) = slot.read().as_ref() {
                    // try_send: never block the hook thread.
                    if let Err(err) = sink.try_send(event) {
                        debug!(?err, "dropping key event (sink full or closed)");
                    }
                }
            }

            // Last, so the engine has already been told about the
            // keystroke and a held key is still replayed behind the
            // correction; returning non-zero is what keeps it from the
            // focused application.
            //
            // Two atomic loads and a comparison, and deliberately no
            // logging: this runs per keystroke, and the one thing worse
            // than a slow hook is a slow hook that writes to disk.
            let swallow = GATE
                .get()
                .and_then(|slot| slot.read().as_ref().map(|g| g.swallow(ours)))
                .unwrap_or(false);
            if swallow {
                return LRESULT(1);
            }
        }
    }

    // Safety: pass through to the next hook in the chain. The first
    // arg is the hook handle; passing a default works per docs (it is
    // ignored for WH_KEYBOARD_LL since Win XP, but we still must call).
    unsafe { CallNextHookEx(HHOOK(std::ptr::null_mut()), code, wparam, lparam) }
}

fn read_modifiers() -> Modifiers {
    fn down(vk: u16) -> bool {
        // Safety: GetAsyncKeyState is a trivial Win32 call.
        unsafe { (GetAsyncKeyState(vk as i32) as u16) & 0x8000 != 0 }
    }
    Modifiers {
        shift: down(VK_SHIFT.0),
        control: down(VK_CONTROL.0),
        alt: down(VK_MENU.0) || down(VK_LMENU.0) || down(VK_RMENU.0),
        meta: down(VK_LWIN.0) || down(VK_RWIN.0),
    }
}
