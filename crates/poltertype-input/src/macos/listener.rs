//! `CGEventTap` listener: attach, translate, forward.

use std::ffi::{c_long, c_void};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::Duration;

use core_foundation::base::TCFType;
use core_foundation::mach_port::CFMachPortRef;
use core_foundation::runloop::{
    CFRunLoop, CFRunLoopAddSource, CFRunLoopRunInMode, CFRunLoopSource, CFRunLoopSourceRef,
    kCFRunLoopCommonModes, kCFRunLoopDefaultMode,
};
use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
    CallbackResult,
};
use crossbeam_channel::Sender;
use tracing::{debug, info, trace};

use super::codes::{flags_changed_direction, mac_keycode_to_sc1};
use super::consts::{EMITTER_TAG, K_CG_EVENT_SOURCE_USER_DATA, K_CG_KEYBOARD_EVENT_KEYCODE};
use super::gate::MacosGate;
use crate::{InputError, InputListener, KeyDirection, KeyEvent, Modifiers};

// ─── Accessibility permission prompt ─────────────────────────────────
//
// `CGEventTapCreate` fails *silently* without Accessibility rights — no
// system dialog. `AXIsProcessTrustedWithOptions` with the prompt option
// is the supported way to ask; without it a first-launch user gets a
// dead tray icon and no explanation.

use core_foundation::dictionary::CFDictionaryRef;
use core_foundation::string::CFStringRef;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
}

fn request_accessibility_prompt() {
    unsafe {
        let key =
            core_foundation::string::CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
        let value = core_foundation::boolean::CFBoolean::true_value();
        let options = core_foundation::dictionary::CFDictionary::from_CFType_pairs(&[(
            key.as_CFType(),
            value.as_CFType(),
        )]);
        let trusted = AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef());
        debug!(trusted, "AXIsProcessTrustedWithOptions(prompt) result");
    }
}

// ─── Listener ────────────────────────────────────────────────────────

static EVENT_SINK: OnceLock<parking_lot::RwLock<Option<Sender<KeyEvent>>>> = OnceLock::new();

static FIRST_EVENT_LOGGED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn sink_slot() -> &'static parking_lot::RwLock<Option<Sender<KeyEvent>>> {
    EVENT_SINK.get_or_init(|| parking_lot::RwLock::new(None))
}

pub struct MacosListener {
    started: bool,
    /// The key gate the tap callback consults on every keystroke.
    /// `None` = observe only, never swallow.
    gate: Option<Arc<MacosGate>>,
}

impl MacosListener {
    pub fn new() -> Self {
        Self {
            started: false,
            gate: None,
        }
    }

    /// Wire the listener to the engine's gate, so the tap callback may
    /// swallow a keystroke instead of only observing it.
    pub fn with_gate(gate: Arc<MacosGate>) -> Self {
        Self {
            started: false,
            gate: Some(gate),
        }
    }
}

impl InputListener for MacosListener {
    fn start(&mut self, sink: Sender<KeyEvent>) -> Result<(), InputError> {
        if self.started {
            return Err(InputError::AlreadyStarted);
        }
        *sink_slot().write() = Some(sink);

        let gate = self.gate.clone();
        let (ready_tx, ready_rx) = crossbeam_channel::bounded::<Result<(), String>>(1);
        thread::Builder::new()
            .name("poltertype-input-macos-tap".into())
            .spawn(move || run_tap_thread(gate, ready_tx))
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
        if let Some(slot) = EVENT_SINK.get() {
            *slot.write() = None;
        }
    }

    fn backend_name(&self) -> &'static str {
        "macos-cg-event-tap"
    }
}

/// Translate one tap event into a [`KeyEvent`], or `None` for events
/// the engine has no use for. Runs inside the tap callback, which must
/// do nothing but this and a `try_send` — see [`TAP_PORT`].
fn to_key_event(ev_type: CGEventType, event: &CGEvent) -> Option<KeyEvent> {
    // `CGEventField` is a `u32` type-alias in core-graphics 0.24, so the
    // documented Apple constants go straight through.
    let vk = event.get_integer_value_field(K_CG_KEYBOARD_EVENT_KEYCODE) as u32;
    let flags = event.get_flags();

    let direction = match ev_type {
        CGEventType::KeyDown => KeyDirection::Press,
        CGEventType::KeyUp => KeyDirection::Release,
        // A modifier moved, and macOS reports no direction of its own:
        // the flags describe the state *after* the change, so the bit
        // of the key that moved tells us which way it went. Keys we
        // don't mirror (Fn, media) yield `None` and are dropped rather
        // than falling through the SC-1 identity mapping into the
        // classifier's "end the word" range.
        CGEventType::FlagsChanged => flags_changed_direction(vk as u16, flags.bits())?,
        _ => return None,
    };

    // Kept apart, not folded together: `shift` is what a replay has to
    // press again, and the lock is still on when it does. macOS reports
    // both as live flags, so neither is ever a guess here.
    Some(KeyEvent {
        vk,
        scancode: mac_keycode_to_sc1(vk as u16),
        direction,
        modifiers: Modifiers {
            shift: flags.contains(CGEventFlags::CGEventFlagShift),
            control: flags.contains(CGEventFlags::CGEventFlagControl),
            alt: flags.contains(CGEventFlags::CGEventFlagAlternate),
            meta: flags.contains(CGEventFlags::CGEventFlagCommand),
            caps: flags.contains(CGEventFlags::CGEventFlagAlphaShift),
        },
        injected: event.get_integer_value_field(K_CG_EVENT_SOURCE_USER_DATA) != 0,
        timestamp_ms: 0,
    })
}

/// The tap's mach port, stashed so the callback can re-enable the tap
/// when the OS disables it — `kCGEventTapDisabledByTimeout` arrives
/// under load even though our callback is a few atomic loads.
///
/// The set runs after `tap.enable()`; a tap disabled inside that gap
/// re-enables against a stale port, which fails toward keys reaching
/// the application — the safe direction.
static TAP_PORT: OnceLock<usize> = OnceLock::new();

fn run_tap_thread(gate: Option<Arc<MacosGate>>, ready_tx: Sender<Result<(), String>>) {
    use core_graphics::event::CGEventTapProxy;

    // The gate only gets to make swallow decisions when the tap is
    // *active* — a listen-only tap's return value is ignored by the
    // window server. A gate disabled by env keeps a listen-only tap.
    let active = gate.as_ref().is_some_and(|g| g.wants_active_tap());
    let gate_for_callback = gate.clone();

    let callback =
        move |_proxy: CGEventTapProxy, ev_type: CGEventType, event: &CGEvent| -> CallbackResult {
            // The OS turned our tap off — put it back. Delivered on the
            // tap itself, not in the key stream.
            if matches!(
                ev_type,
                CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput
            ) {
                if let Some(port) = TAP_PORT.get() {
                    tracing::warn!(?ev_type, "event tap disabled by the OS; re-enabling");
                    // Safety: the port belongs to our live tap.
                    unsafe { CGEventTapEnable(*port as CFMachPortRef, true) };
                }
                return CallbackResult::Keep;
            }

            if let Some(ev_out) = to_key_event(ev_type, event) {
                if let Some(slot) = EVENT_SINK.get() {
                    if let Some(sink) = slot.read().as_ref() {
                        if !FIRST_EVENT_LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                            debug!("first macOS key event delivered to engine");
                        }
                        trace!(
                            scancode = ev_out.scancode,
                            direction = ?ev_out.direction,
                            shift = ev_out.modifiers.shift,
                            ctrl = ev_out.modifiers.control,
                            alt = ev_out.modifiers.alt,
                            meta = ev_out.modifiers.meta,
                            injected = ev_out.injected,
                            "mac key"
                        );
                        if let Err(err) = sink.try_send(ev_out) {
                            debug!(?err, "dropping macOS key event");
                        }
                    }
                }

                // While a correction burst is on the wire the user's
                // keystrokes are swallowed here — the engine already has
                // them and replays them behind the correction. Our own
                // emissions are stamped and must always pass, or the
                // correction swallows itself. `FlagsChanged` is never
                // swallowed: holding one modifier edge and not its
                // counterpart leaves the system state stuck.
                if let Some(g) = gate_for_callback.as_ref() {
                    if matches!(ev_type, CGEventType::KeyDown | CGEventType::KeyUp) {
                        let ours = event.get_integer_value_field(K_CG_EVENT_SOURCE_USER_DATA)
                            == EMITTER_TAG;
                        if g.swallow(ours) {
                            trace!(scancode = ev_out.scancode, "key held by gate");
                            return CallbackResult::Drop;
                        }
                    }
                }
            }
            CallbackResult::Keep
        };

    let tap = match core_graphics::event::CGEventTap::new(
        CGEventTapLocation::Session,
        CGEventTapPlacement::HeadInsertEventTap,
        if active {
            CGEventTapOptions::Default
        } else {
            CGEventTapOptions::ListenOnly
        },
        // `FlagsChanged` is how macOS reports a modifier press or
        // release — there is no KeyDown for Shift. Without it
        // `held_modifiers` (and so `release_modifiers`) goes stale
        // between ordinary keystrokes.
        vec![
            CGEventType::KeyDown,
            CGEventType::KeyUp,
            CGEventType::FlagsChanged,
        ],
        callback,
    ) {
        Ok(t) => t,
        Err(()) => {
            request_accessibility_prompt();
            let _ = ready_tx.send(Err(
                "CGEventTapCreate failed (likely missing Accessibility permission)".into(),
            ));
            return;
        }
    };

    // Safety: hand the mach port to a CFRunLoopSource. The source
    // owns a +1 refcount we wrap into Drop via CFRunLoopSource.
    let source = unsafe {
        let mach_port_ref: CFMachPortRef = tap.mach_port().as_concrete_TypeRef();
        let src_ref = CFMachPortCreateRunLoopSource(std::ptr::null(), mach_port_ref, 0);
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
    let _ = TAP_PORT.set(tap.mach_port().as_concrete_TypeRef() as usize);
    if let Some(g) = gate.as_ref() {
        g.set_tap_running(true);
    }

    let _ = ready_tx.send(Ok(()));

    loop {
        // Safety: standard CFRunLoop call. Must run the loop in a real
        // mode (kCFRunLoopDefaultMode is in the common-mode set the
        // tap source was added to) — passing kCFRunLoopCommonModes as
        // the *run* mode is legal per the docs but on macOS 15 the tap
        // source never fires that way, so the callback starves.
        unsafe {
            let _ = CFRunLoopRunInMode(kCFRunLoopDefaultMode, 60.0, 0);
        }
        if EVENT_SINK.get().map(|s| s.read().is_none()).unwrap_or(true) {
            break;
        }
    }
    if let Some(g) = gate.as_ref() {
        g.set_tap_running(false);
    }
    info!("macOS CGEventTap thread exiting");
}

// ─── Direct FFI: only the things core-foundation 0.10 doesn't expose ──
//
// `CFMachPortCreateRunLoopSource` moves between modules across
// core-foundation versions; declaring it here decouples us from that.

type CFAllocatorRef = *const c_void;
type CFIndex = c_long;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFMachPortCreateRunLoopSource(
        allocator: CFAllocatorRef,
        port: CFMachPortRef,
        order: CFIndex,
    ) -> CFRunLoopSourceRef;
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
}
