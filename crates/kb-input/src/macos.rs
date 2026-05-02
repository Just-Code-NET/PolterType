//! macOS keyboard listener + emitter.
//!
//! ## Listener
//!
//! Built on `CGEventTapCreate(kCGSessionEventTap, …, listenOnly)`,
//! attached to the `CFRunLoop` of a dedicated thread. macOS requires
//! the calling app to be granted **Accessibility** in
//! System Settings → Privacy & Security → Accessibility. We surface
//! that requirement to the user via the tray onboarding banner; if
//! the tap fails to attach (typical first-launch state), `start()`
//! returns `InputError::Os` so the engine gracefully degrades.
//!
//! ## Emitter
//!
//! `CGEventPost` with `CGEventKeyboardSetUnicodeString` — same
//! layout-independent contract as Windows' `KEYEVENTF_UNICODE`.
//!
//! > **Status:** written from Apple's documented behaviour and
//! > tested only via `cargo check` on macOS CI. Runtime bugs are
//! > expected in v0.1.x; the surface area is small enough that we'll
//! > iron them out as users report them.

#![allow(unused_imports, dead_code)] // macOS-only; see DECISIONS for status.

use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

use core_foundation::base::TCFType;
use core_foundation::mach_port::CFMachPortRef;
use core_foundation::runloop::{
    CFRunLoop, CFRunLoopAddSource, CFRunLoopRunInMode, CFRunLoopSource, CFRunLoopSourceRef,
    kCFRunLoopCommonModes,
};
use core_graphics::event::{
    CGEvent, CGEventField, CGEventFlags, CGEventTapLocation, CGEventTapOptions,
    CGEventTapPlacement, CGEventType, CGKeyCode,
};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use crossbeam_channel::Sender;
use tracing::{debug, info, warn};

use crate::{InputError, InputListener, KeyDirection, KeyEmitter, KeyEvent, Modifiers};

// ─── Listener ────────────────────────────────────────────────────────

static EVENT_SINK: OnceLock<parking_lot::RwLock<Option<Sender<KeyEvent>>>> = OnceLock::new();

fn sink_slot() -> &'static parking_lot::RwLock<Option<Sender<KeyEvent>>> {
    EVENT_SINK.get_or_init(|| parking_lot::RwLock::new(None))
}

pub struct MacosListener {
    started: bool,
}

impl MacosListener {
    pub fn new() -> Self {
        Self { started: false }
    }
}

impl InputListener for MacosListener {
    fn start(&mut self, sink: Sender<KeyEvent>) -> Result<(), InputError> {
        if self.started {
            return Err(InputError::AlreadyStarted);
        }
        *sink_slot().write() = Some(sink);

        // Spawn a dedicated thread that owns a CFRunLoop and the tap.
        let (ready_tx, ready_rx) = crossbeam_channel::bounded::<Result<(), String>>(1);
        thread::Builder::new()
            .name("kb-input-macos-tap".into())
            .spawn(move || run_tap_thread(ready_tx))
            .map_err(|e| InputError::Os(format!("spawn tap thread: {e}")))?;

        match ready_rx.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(())) => {
                self.started = true;
                info!("macOS CGEventTap attached");
                Ok(())
            }
            Ok(Err(reason)) => Err(InputError::Os(reason)),
            Err(_) => Err(InputError::Os("CGEventTap setup timed out".into())),
        }
    }

    fn stop(&mut self) {
        // The tap thread parks itself on its CFRunLoop forever; v0.1
        // simply lets it die with the process. A clean shutdown via
        // `CFRunLoopStop` is a Phase 5.x polish.
        if let Some(slot) = EVENT_SINK.get() {
            *slot.write() = None;
        }
    }

    fn backend_name(&self) -> &'static str {
        "macos-cg-event-tap"
    }
}

fn run_tap_thread(ready_tx: Sender<Result<(), String>>) {
    use core_graphics::event::CGEventTapProxy;

    let mask = (1u64 << CGEventType::KeyDown as u64) | (1u64 << CGEventType::KeyUp as u64);

    let callback =
        |_proxy: CGEventTapProxy, ev_type: CGEventType, event: &CGEvent| -> Option<CGEvent> {
            let direction = match ev_type {
                CGEventType::KeyDown => Some(KeyDirection::Press),
                CGEventType::KeyUp => Some(KeyDirection::Release),
                _ => None,
            };
            if let Some(direction) = direction {
                let vk = event.get_integer_value_field(CGEventField::KeyboardEventKeycode) as u32;
                let scancode = mac_keycode_to_sc1(vk as u16);
                let flags = event.get_flags();
                // Heuristic: events posted by CGEventPost typically have a
                // non-zero EventSourceUserData when tagged by another tool;
                // for our own corrections we set this in a future step.
                let injected =
                    event.get_integer_value_field(CGEventField::EventSourceUserData) != 0;

                let ev_out = KeyEvent {
                    vk,
                    scancode,
                    direction,
                    modifiers: Modifiers {
                        shift: flags.contains(CGEventFlags::CGEventFlagShift),
                        control: flags.contains(CGEventFlags::CGEventFlagControl),
                        alt: flags.contains(CGEventFlags::CGEventFlagAlternate),
                        meta: flags.contains(CGEventFlags::CGEventFlagCommand),
                    },
                    injected,
                    timestamp_ms: 0,
                };
                if let Some(slot) = EVENT_SINK.get() {
                    if let Some(sink) = slot.read().as_ref() {
                        if let Err(err) = sink.try_send(ev_out) {
                            debug!(?err, "dropping macOS key event");
                        }
                    }
                }
            }
            // Pass-through; we listen but don't suppress.
            Some(event.clone())
        };

    let tap = match core_graphics::event::CGEventTap::new(
        CGEventTapLocation::Session,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::ListenOnly,
        vec![CGEventType::KeyDown, CGEventType::KeyUp],
        callback,
    ) {
        Ok(t) => t,
        Err(()) => {
            let _ = ready_tx.send(Err(
                "CGEventTapCreate failed (likely missing Accessibility permission)".into(),
            ));
            return;
        }
    };

    // Safety: we hand the mach port to a CFRunLoopSource; both objects
    // live as long as this thread.
    let source = unsafe {
        let mach_port_ref: CFMachPortRef = tap.mach_port().as_concrete_TypeRef();
        let src_ref: CFRunLoopSourceRef = core_foundation::runloop::CFMachPortCreateRunLoopSource(
            std::ptr::null(),
            mach_port_ref,
            0,
        );
        if src_ref.is_null() {
            let _ = ready_tx.send(Err("CFMachPortCreateRunLoopSource returned null".into()));
            return;
        }
        CFRunLoopSource::wrap_under_create_rule(src_ref)
    };

    let run_loop = CFRunLoop::get_current();
    unsafe {
        CFRunLoopAddSource(
            run_loop.as_concrete_TypeRef(),
            source.as_concrete_TypeRef(),
            kCFRunLoopCommonModes,
        );
    }
    tap.enable();

    let _ = ready_tx.send(Ok(()));

    // Park. CFRunLoopRun blocks; the closure invokes our callback.
    loop {
        // A 60-second timeout lets the loop occasionally check
        // process-level shutdown signals (added in Phase 5.x).
        unsafe {
            let _ = CFRunLoopRunInMode(kCFRunLoopCommonModes, 60.0, 0);
        }
        if EVENT_SINK.get().map(|s| s.read().is_none()).unwrap_or(true) {
            break;
        }
    }
    info!("macOS CGEventTap thread exiting");
}

// ─── Emitter ─────────────────────────────────────────────────────────

pub struct MacosEmitter;

impl MacosEmitter {
    pub fn new() -> Self {
        Self
    }
}

const KVK_DELETE: CGKeyCode = 51; // = "Backspace" / kVK_Delete on Apple keyboards.

impl KeyEmitter for MacosEmitter {
    fn send_backspaces(&self, n: usize) -> Result<(), InputError> {
        if n == 0 {
            return Ok(());
        }
        let src = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .map_err(|()| InputError::Os("CGEventSource::new failed".into()))?;
        for _ in 0..n {
            let down = CGEvent::new_keyboard_event(src.clone(), KVK_DELETE, true)
                .map_err(|()| InputError::Os("CGEvent::new_keyboard_event(down) failed".into()))?;
            down.post(CGEventTapLocation::HID);
            let up = CGEvent::new_keyboard_event(src.clone(), KVK_DELETE, false)
                .map_err(|()| InputError::Os("CGEvent::new_keyboard_event(up) failed".into()))?;
            up.post(CGEventTapLocation::HID);
        }
        Ok(())
    }

    fn send_text(&self, text: &str) -> Result<(), InputError> {
        if text.is_empty() {
            return Ok(());
        }
        let src = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .map_err(|()| InputError::Os("CGEventSource::new failed".into()))?;

        // One synthetic key-down/up per char, with the unicode string
        // attached. `set_string_from_utf16_unchecked` is the modern
        // helper; we use the existing string-set API of core-graphics.
        for c in text.chars() {
            let utf16: Vec<u16> = c.encode_utf16(&mut [0u16; 2]).iter().copied().collect();
            // Use a key code of 0 — Apple ignores it once a Unicode
            // string is attached.
            let down = CGEvent::new_keyboard_event(src.clone(), 0, true)
                .map_err(|()| InputError::Os("CGEvent::new_keyboard_event failed".into()))?;
            down.set_string_from_utf16_unchecked(&utf16);
            down.post(CGEventTapLocation::HID);

            let up = CGEvent::new_keyboard_event(src.clone(), 0, false)
                .map_err(|()| InputError::Os("CGEvent::new_keyboard_event failed".into()))?;
            up.set_string_from_utf16_unchecked(&utf16);
            up.post(CGEventTapLocation::HID);
        }
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "macos-cg-event-post"
    }
}

// ─── Apple → Win SC Set-1 keycode mapping ────────────────────────────
//
// Apple uses its own keycode space (kVK_*). We normalise to Win SC1
// so the layout-mapping TOMLs and WordBuffer classifier work
// unchanged. Only the alphanumeric / punctuation / nav rows we care
// about are mapped here; everything else gets passed through as-is
// (the buffer will simply not produce a translation for those).
fn mac_keycode_to_sc1(kvk: u16) -> u32 {
    match kvk {
        // Letters
        0x00 => 0x1E, // A
        0x01 => 0x1F, // S
        0x02 => 0x20, // D
        0x03 => 0x21, // F
        0x04 => 0x23, // H
        0x05 => 0x22, // G
        0x06 => 0x2C, // Z
        0x07 => 0x2D, // X
        0x08 => 0x2E, // C
        0x09 => 0x2F, // V
        0x0B => 0x30, // B
        0x0C => 0x10, // Q
        0x0D => 0x11, // W
        0x0E => 0x12, // E
        0x0F => 0x13, // R
        0x10 => 0x15, // Y
        0x11 => 0x14, // T
        0x1F => 0x18, // O
        0x20 => 0x16, // U
        0x22 => 0x17, // I
        0x23 => 0x19, // P
        0x25 => 0x26, // L
        0x26 => 0x24, // J
        0x28 => 0x25, // K
        0x2D => 0x31, // N
        0x2E => 0x32, // M
        // Numbers
        0x12 => 0x02, // 1
        0x13 => 0x03, // 2
        0x14 => 0x04, // 3
        0x15 => 0x05, // 4
        0x17 => 0x06, // 5
        0x16 => 0x07, // 6
        0x1A => 0x08, // 7
        0x1C => 0x09, // 8
        0x19 => 0x0A, // 9
        0x1D => 0x0B, // 0
        // Common boundaries / nav
        0x24 => 0x1C, // Return
        0x30 => 0x0F, // Tab
        0x31 => 0x39, // Space
        0x33 => 0x0E, // Delete (= Backspace)
        0x35 => 0x01, // Esc
        0x2B => 0x33, // Comma
        0x2F => 0x34, // Period
        0x2C => 0x35, // Slash
        0x29 => 0x27, // ;
        0x27 => 0x28, // '
        0x21 => 0x1A, // [
        0x1E => 0x1B, // ]
        0x2A => 0x2B, // backslash
        0x32 => 0x29, // backtick
        0x18 => 0x0D, // =
        0x1B => 0x0C, // -
        _ => kvk as u32,
    }
}
