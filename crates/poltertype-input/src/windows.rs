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

use crossbeam_channel::Sender;
use tracing::{debug, error, info, warn};
use windows::Win32::Foundation::{HMODULE, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, SendInput, VIRTUAL_KEY, VK_BACK, VK_CONTROL, VK_LCONTROL,
    VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, MSG,
    PostThreadMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::{InputError, InputListener, KeyDirection, KeyEmitter, KeyEvent, Modifiers};

/// Sender shared with the C-callable hook procedure. There can only be
/// one global keyboard hook per process at a time. `parking_lot::RwLock`
/// gives us a non-poisoning shared cell without pulling in any global
/// allocator dance.
static EVENT_SINK: OnceLock<parking_lot::RwLock<Option<Sender<KeyEvent>>>> = OnceLock::new();

fn sink_slot() -> &'static parking_lot::RwLock<Option<Sender<KeyEvent>>> {
    EVENT_SINK.get_or_init(|| parking_lot::RwLock::new(None))
}

pub struct WindowsListener {
    worker: Option<WorkerHandle>,
}

struct WorkerHandle {
    join: JoinHandle<()>,
    thread_id: u32,
    stopping: std::sync::Arc<AtomicBool>,
}

impl WindowsListener {
    pub fn new() -> Self {
        Self { worker: None }
    }
}

impl InputListener for WindowsListener {
    fn start(&mut self, sink: Sender<KeyEvent>) -> Result<(), InputError> {
        if self.worker.is_some() {
            return Err(InputError::AlreadyStarted);
        }

        *sink_slot().write() = Some(sink);

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
            // LLKHF_INJECTED = 0x10, LLKHF_LOWER_IL_INJECTED = 0x02
            let injected = (kb.flags.0 & 0x12) != 0;
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

// ─── KeyEmitter (SendInput) ──────────────────────────────────────────

pub struct WindowsEmitter;

impl WindowsEmitter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WindowsEmitter {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyEmitter for WindowsEmitter {
    fn send_backspaces(&self, n: usize) -> Result<(), InputError> {
        if n == 0 {
            return Ok(());
        }
        let mut events: Vec<INPUT> = Vec::with_capacity(n * 2);
        for _ in 0..n {
            events.push(make_vk_input(VK_BACK, false));
            events.push(make_vk_input(VK_BACK, true));
        }
        send_inputs(&events)
    }

    fn send_text(&self, text: &str) -> Result<(), InputError> {
        if text.is_empty() {
            return Ok(());
        }
        let mut events: Vec<INPUT> = Vec::with_capacity(text.len() * 2);
        // Encode each char as UTF-16; non-BMP codepoints take two
        // INPUT events (high + low surrogate). Each codepoint is sent
        // both as a key-down and key-up.
        for c in text.chars() {
            let mut buf = [0u16; 2];
            for &unit in c.encode_utf16(&mut buf).iter() {
                events.push(make_unicode_input(unit, false));
                events.push(make_unicode_input(unit, true));
            }
        }
        send_inputs(&events)
    }

    fn release_modifiers(&self, held: Modifiers) -> Result<(), InputError> {
        // Both sides of each: `read_modifiers` reports "shift is down",
        // not which shift, and `SendInput` of a key-up for a key that
        // is already up is a no-op.
        //
        // This clears the *logical* modifier state applications read
        // from the message queue, which is what decides whether our
        // replay arrives as text or as a burst of shortcuts. The user's
        // physical key stays down; their own release lands on an
        // already-up key and is ignored, and we deliberately do not
        // press the modifiers back — re-pressing one they have
        // meanwhile let go of would leave it stuck down.
        let mut events: Vec<INPUT> = Vec::new();
        for (down, keys) in [
            (held.control, [VK_LCONTROL, VK_RCONTROL].as_slice()),
            (held.shift, [VK_LSHIFT, VK_RSHIFT].as_slice()),
            (held.alt, [VK_LMENU, VK_RMENU].as_slice()),
            (held.meta, [VK_LWIN, VK_RWIN].as_slice()),
        ] {
            if down {
                events.extend(keys.iter().map(|&vk| make_vk_input(vk, true)));
            }
        }
        if events.is_empty() {
            return Ok(());
        }
        send_inputs(&events)
    }

    fn backend_name(&self) -> &'static str {
        "windows-sendinput"
    }
}

fn make_vk_input(vk: VIRTUAL_KEY, key_up: bool) -> INPUT {
    let flags = if key_up {
        KEYEVENTF_KEYUP
    } else {
        KEYBD_EVENT_FLAGS(0)
    };
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn make_unicode_input(unit: u16, key_up: bool) -> INPUT {
    let mut flags = KEYEVENTF_UNICODE;
    if key_up {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: unit,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send_inputs(events: &[INPUT]) -> Result<(), InputError> {
    if events.is_empty() {
        return Ok(());
    }
    // Safety: SendInput requires a contiguous INPUT slice; we pass
    // exactly that. Returns the number actually inserted.
    let n = unsafe { SendInput(events, std::mem::size_of::<INPUT>() as i32) };
    if n as usize != events.len() {
        return Err(InputError::Os(format!(
            "SendInput sent {n}/{} events",
            events.len()
        )));
    }
    Ok(())
}
