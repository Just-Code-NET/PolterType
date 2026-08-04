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
};
use crossbeam_channel::Sender;
use tracing::{debug, info, trace};

use super::codes::{flags_changed_direction, mac_keycode_to_sc1};
use super::consts::{EMITTER_TAG, K_CG_EVENT_SOURCE_USER_DATA, K_CG_KEYBOARD_EVENT_KEYCODE};
use super::gate::MacosGate;
use crate::{InputError, InputListener, KeyDirection, KeyEvent, Modifiers};

// ─── Accessibility permission prompt ─────────────────────────────────
//
// `CGEventTapCreate` fails *silently* when the app lacks Accessibility
// rights — no system dialog. The supported way to ask is
// `AXIsProcessTrustedWithOptions({ kAXTrustedCheckOptionPrompt: true })`,
// which drops the app into System Settings → Privacy & Security →
// Accessibility and shows the "PolterType would like to control this
// computer" alert. We call it when the tap fails to attach so a
// first-launch user gets the prompt instead of a dead tray icon.

use core_foundation::dictionary::CFDictionaryRef;
use core_foundation::string::CFStringRef;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
}

/// Check Accessibility trust; prompt the user when not yet trusted.
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
    /// `None` = observe-only, the pre-gate behaviour.
    gate: Option<Arc<MacosGate>>,
}

impl MacosListener {
    pub fn new() -> Self {
        Self {
            started: false,
            gate: None,
        }
    }

    /// Wire the listener to the gate the engine holds, so the tap
    /// callback can swallow a keystroke instead of only observing it.
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
/// the engine has no use for.
///
/// Split out of the callback so the direction/flag rules it leans on
/// stay in one place; the callback itself must do nothing but this and
/// a `try_send`.
fn to_key_event(ev_type: CGEventType, event: &CGEvent) -> Option<KeyEvent> {
    // `CGEventField` is a `u32` type-alias in core-graphics 0.24, so we
    // feed the documented Apple constants straight through
    // `get_integer_value_field`.
    let vk = event.get_integer_value_field(K_CG_KEYBOARD_EVENT_KEYCODE) as u32;
    let flags = event.get_flags();

    let direction = match ev_type {
        CGEventType::KeyDown => KeyDirection::Press,
        CGEventType::KeyUp => KeyDirection::Release,
        // A modifier moved. macOS reports no direction of its own: the
        // flags describe the state *after* the change, so the bit
        // belonging to the key that moved tells us which way it went.
        // Keys we don't mirror (Fn, media) yield `None` and are dropped
        // rather than falling through the SC-1 identity mapping into
        // the classifier's "end the word" range.
        CGEventType::FlagsChanged => flags_changed_direction(vk as u16, flags.bits())?,
        _ => return None,
    };

    // Fold Caps Lock into the shift bit the way the X11 backend does:
    // caps-on + no Shift = uppercase, caps-on + held Shift = lowercase.
    // The engine's all-caps and replay logic rely on this combined bit.
    let shift = flags.contains(CGEventFlags::CGEventFlagShift)
        ^ flags.contains(CGEventFlags::CGEventFlagAlphaShift);

    Some(KeyEvent {
        vk,
        scancode: mac_keycode_to_sc1(vk as u16),
        direction,
        modifiers: Modifiers {
            shift,
            control: flags.contains(CGEventFlags::CGEventFlagControl),
            alt: flags.contains(CGEventFlags::CGEventFlagAlternate),
            meta: flags.contains(CGEventFlags::CGEventFlagCommand),
        },
        injected: event.get_integer_value_field(K_CG_EVENT_SOURCE_USER_DATA) != 0,
        timestamp_ms: 0,
    })
}

/// The tap's mach port, stashed after creation so the callback can
/// re-enable the tap if the OS disables it (`kCGEventTapDisabledByTimeout`
/// arrives when a callback overruns its budget — ours is a few atomic
/// loads, but an OS under load can still decide; coming back to life
/// beats staying deaf).
static TAP_PORT: OnceLock<usize> = OnceLock::new();

fn run_tap_thread(gate: Option<Arc<MacosGate>>, ready_tx: Sender<Result<(), String>>) {
    use core_graphics::event::CGEventTapProxy;

    // The gate only gets to make swallow decisions when the tap is
    // *active* — a listen-only tap's return value is ignored by the
    // window server. Disabled-by-env gates keep the old listen-only tap.
    let active = gate.as_ref().is_some_and(|g| g.wants_active_tap());
    let gate_for_callback = gate.clone();

    let callback =
        move |_proxy: CGEventTapProxy, ev_type: CGEventType, event: &CGEvent| -> Option<CGEvent> {
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
                return Some(event.clone());
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

                // The key gate: while a correction burst is on the
                // wire, the user's keystrokes are swallowed here (the
                // engine already has them — it replays them behind the
                // correction). Our own emissions are stamped and must
                // always pass, or the correction swallows itself.
                // `FlagsChanged` events never get swallowed: holding a
                // modifier edge but not its counterpart would leave the
                // system modifier state stuck.
                if let Some(g) = gate_for_callback.as_ref() {
                    if matches!(ev_type, CGEventType::KeyDown | CGEventType::KeyUp) {
                        let ours = event.get_integer_value_field(K_CG_EVENT_SOURCE_USER_DATA)
                            == EMITTER_TAG;
                        if g.swallow(ours) {
                            trace!(scancode = ev_out.scancode, "key held by gate");
                            return None;
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
        if active {
            CGEventTapOptions::Default
        } else {
            CGEventTapOptions::ListenOnly
        },
        // `FlagsChanged` is how macOS reports a modifier press or
        // release — there is no KeyDown for Shift. Subscribing gives
        // the engine the same discrete modifier stream the Windows and
        // Linux backends produce, which is what `held_modifiers` (and
        // therefore `release_modifiers`) needs to stay accurate between
        // ordinary keystrokes. Cost is one extra event per modifier
        // edge; the callback stays a translate-and-send.
        vec![
            CGEventType::KeyDown,
            CGEventType::KeyUp,
            CGEventType::FlagsChanged,
        ],
        callback,
    ) {
        Ok(t) => t,
        Err(()) => {
            // Trigger the system Accessibility prompt so the user has
            // a one-click path to System Settings, then report the
            // failure as before.
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
        let mach_port_ref: CFMachPortRef = tap.mach_port.as_concrete_TypeRef();
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
    let _ = TAP_PORT.set(tap.mach_port.as_concrete_TypeRef() as usize);
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
// `CFMachPortCreateRunLoopSource` is the one bit of glue that's not
// re-exported reliably across core-foundation crate versions. Declare
// it ourselves so we don't depend on whichever module the active
// version ships it from.

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
