//! `EvdevGate` — holds the user's keystrokes back from applications
//! while a correction burst is on the wire.
//!
//! The compositor interleaves whatever the user types into our burst
//! and counting cannot undo that afterwards, so the only real fix is
//! `EVIOCGRAB` — the evdev equivalent of a Windows low-level hook
//! swallowing events. The grabbed devices are still read, so the engine
//! sees every keystroke and replays them behind the correction, in
//! order.
//!
//! Two things make this safe enough to enable by default: **the device
//! thread owns the grab**, dropping it after [`MAX_HOLD`] whatever the
//! engine is doing, and the kernel releases it if the process dies; and
//! it **refuses to run where it would gag us instead**, see
//! [`EvdevGate::probe_availability`].

use super::*;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tracing::{debug, info, warn};

/// What the key gate needs of a device: the two grab syscalls and the
/// bookkeeping around them. `OpenDevice` is the real implementation;
/// tests supply a fake that can fail, be slow, or report itself as a
/// mouse, so the gate's decisions are testable without a keyboard.
pub(crate) trait GateDevice {
    fn grab(&mut self) -> std::io::Result<()>;
    fn ungrab(&mut self) -> std::io::Result<()>;
    fn state(&self) -> &GateState;
    fn state_mut(&mut self) -> &mut GateState;
    /// Identifies the device in log lines.
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

pub struct EvdevGate {
    /// The engine wants the user's keys held right now.
    want: AtomicBool,
    /// The device thread has actually taken the grabs.
    held: AtomicBool,
    /// Grabbing is safe on this stack — see `probe_availability`.
    available: AtomicBool,
    /// Milliseconds since `origin` past which the device thread drops
    /// the hold unconditionally.
    deadline_ms: AtomicU64,
    /// Bumped by every `hold()`, so the device thread knows to give
    /// each device one fresh grab attempt and no more.
    epoch: AtomicU64,
    /// The epoch whose emitter-proxy re-verification already ran —
    /// see the top of [`Self::service`]'s hold path.
    verified_epoch: AtomicU64,
    origin: Instant,
}

/// Has this device produced anything lately? The window is generous —
/// it only has to separate "the keyboard being typed on" from devices
/// that have been silent since boot.
fn recently_used(st: &GateState) -> bool {
    st.last_event
        .is_some_and(|t| t.elapsed() <= RECENT_USE_WINDOW)
}

impl Default for EvdevGate {
    fn default() -> Self {
        Self::new()
    }
}

impl EvdevGate {
    pub(crate) fn new() -> Self {
        Self {
            want: AtomicBool::new(false),
            held: AtomicBool::new(false),
            available: AtomicBool::new(false),
            deadline_ms: AtomicU64::new(0),
            epoch: AtomicU64::new(1),
            verified_epoch: AtomicU64::new(0),
            origin: Instant::now(),
        }
    }

    fn now_ms(&self) -> u64 {
        self.origin.elapsed().as_millis() as u64
    }

    pub(crate) fn available(&self) -> bool {
        self.available.load(Ordering::Acquire)
    }

    /// Decide whether grabbing can work on this stack at all.
    ///
    /// The hazard is an input remapper. Those hold every keyboard
    /// exclusively — ours included — and re-emit through one virtual
    /// keyboard, which then carries *our* injected keys too, so
    /// grabbing it would block the very correction we are protecting.
    /// The symptom is corrections that silently do nothing.
    ///
    /// The tell is cheap: if we can grab our own emitter, nobody is
    /// proxying it and grabbing the user's keyboards is safe. `EBUSY`
    /// means something sits between us and the compositor and the gate
    /// stays off. Excluding our device in the remapper's config flips
    /// the probe on restart — `docs/PERMISSIONS.md` has the keyd
    /// one-liner.
    pub(crate) fn probe_availability(&self) {
        let found = evdev::enumerate().find(|(_, dev)| dev.name() == Some(EMITTER_DEVICE_NAME));
        let ok = match found {
            Some((path, mut dev)) => match dev.grab() {
                Ok(()) => {
                    let _ = dev.ungrab();
                    debug!(?path, "key gate: our emitter is unproxied");
                    true
                }
                Err(e) => {
                    info!(
                        ?e,
                        "key gate off: an input remapper holds our emitter, so holding the \
                         keyboard would block our own corrections too — see docs/PERMISSIONS.md"
                    );
                    false
                }
            },
            None => {
                debug!("key gate off: no emitter device to check (uinput unavailable?)");
                false
            }
        };
        self.available.store(ok, Ordering::Release);
        if ok {
            info!("key gate ready: keystrokes are held back during corrections");
        }
    }

    /// Ask for the hold and wait briefly for the device thread to take
    /// it. Returns whether the user's keys are actually being held —
    /// `false` means the correction must assume its burst can be
    /// interleaved, exactly as before the gate existed.
    pub(crate) fn hold(&self) -> bool {
        if !self.available() {
            return false;
        }
        // A grab left over from a previous correction would make this
        // one treat held keystrokes as already on screen and delete
        // text that isn't there. Should never happen — release waits
        // for confirmation — so say so if it does.
        if self.held.load(Ordering::Acquire) {
            warn!("key gate: a previous hold was still in force; releasing it first");
            self.release();
        }
        self.deadline_ms.store(
            self.now_ms() + MAX_HOLD.as_millis() as u64,
            Ordering::Release,
        );
        self.epoch.fetch_add(1, Ordering::AcqRel);
        self.want.store(true, Ordering::Release);
        let until = Instant::now() + HOLD_HANDSHAKE;
        while Instant::now() < until {
            if self.held.load(Ordering::Acquire) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        // The device thread never got to it (or had nothing it could
        // grab). Withdraw the request rather than leave it armed.
        self.want.store(false, Ordering::Release);
        warn!("key gate: hold not taken within the handshake window; proceeding unheld");
        false
    }

    /// Stop holding, and wait for the device thread to confirm the grab
    /// is gone. The wait draws the line: everything read off the key
    /// stream before this returns was held back and must be typed out,
    /// everything after reaches the application by itself.
    pub(crate) fn release(&self) {
        self.want.store(false, Ordering::Release);
        let until = Instant::now() + RELEASE_HANDSHAKE;
        while Instant::now() < until {
            if !self.held.load(Ordering::Acquire) {
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        warn!("key gate: release not confirmed within the handshake window");
    }

    /// Called from the device thread on every poll: takes and drops the
    /// grabs, and enforces the watchdog. This is the only place the
    /// devices are touched, which is what keeps ownership simple —
    /// everyone else just flips atomics.
    pub(crate) fn service<D: GateDevice>(&self, devices: &mut [D]) {
        let expired = self.now_ms() >= self.deadline_ms.load(Ordering::Acquire);
        let want = self.want.load(Ordering::Acquire) && !expired;

        if want {
            let epoch = self.epoch.load(Ordering::Acquire);
            // Once per hold, re-verify that our own emitter is still
            // unproxied. The startup probe races the remapper's
            // asynchronous grab of a freshly created device, and losing
            // that race is catastrophic: with keyd proxying our emitter,
            // holding its virtual keyboard funnels every input path into
            // this process and the session's input dies until the
            // watchdog blinks. `EBUSY` turns the gate off until restart.
            // No emitter in the list yet means no rescan has picked it
            // up — proceed on the startup verdict.
            if self.verified_epoch.swap(epoch, Ordering::AcqRel) != epoch {
                if let Some(own) = devices.iter_mut().find(|d| d.state().is_ours) {
                    match own.grab() {
                        Ok(()) => {
                            let _ = own.ungrab();
                        }
                        Err(e) => {
                            warn!(
                                dev = %own.label(),
                                ?e,
                                "key gate off: an input remapper grabbed our emitter after \
                                 startup — holding the keyboard now would funnel every \
                                 input path into us; corrections proceed unheld"
                            );
                            self.available.store(false, Ordering::Release);
                            self.want.store(false, Ordering::Release);
                            self.held.store(false, Ordering::Release);
                            return;
                        }
                    }
                }
            }
            let mut taken = 0usize;
            for od in devices.iter_mut() {
                if od.state().grabbed {
                    taken += 1;
                    continue;
                }
                // Never our own emitter (grabbing that would hold back
                // the correction we are about to type), and never a
                // device this hold has already failed on.
                if od.state().is_ours || od.state().tried_epoch == epoch {
                    continue;
                }
                // Only keyboards, and only ones in recent use: giving a
                // device back costs 13–25 ms, so holding the mouse, the
                // lid switch and three idle HID endpoints turned a
                // correction into a ~100 ms stall in the very thread
                // that has to notice the user typing.
                if !od.state().is_keyboard || !recently_used(od.state()) {
                    continue;
                }
                od.state_mut().tried_epoch = epoch;
                match od.grab() {
                    Ok(()) => {
                        od.state_mut().grabbed = true;
                        taken += 1;
                    }
                    // A device we cannot grab (a remapper already holds
                    // it) simply isn't a path the user's keys take to
                    // the compositor — the grabbable one is.
                    Err(e) => debug!(dev = %od.label(), ?e, "key gate: device not grabbable"),
                }
            }
            // Re-run on every poll rather than once per hold, so a
            // keyboard the rescan picks up mid-hold is covered too.
            if taken > 0 && !self.held.swap(true, Ordering::AcqRel) {
                debug!(devices = taken, "key gate: holding");
            }
            self.held.store(taken > 0, Ordering::Release);
            if taken == 0 {
                self.want.store(false, Ordering::Release);
            }
        } else {
            let mut released = false;
            for od in devices.iter_mut().filter(|od| od.state().grabbed) {
                if let Err(e) = od.ungrab() {
                    warn!(dev = %od.label(), ?e, "key gate: ungrab failed");
                }
                od.state_mut().grabbed = false;
                released = true;
            }
            if released {
                self.held.store(false, Ordering::Release);
                debug!("key gate: released");
            }
            if expired && self.want.swap(false, Ordering::AcqRel) {
                warn!("key gate watchdog fired — hold released without the engine asking");
            }
        }
    }

    /// Availability is normally decided by probing the emitter device,
    /// which a test cannot arrange.
    #[cfg(test)]
    pub(crate) fn mark_available_for_test(&self) {
        self.available.store(true, Ordering::Release);
    }

    /// Arm a hold without the device-thread handshake — a test has no
    /// device thread to shake hands with.
    #[cfg(test)]
    pub(crate) fn hold_for_test(&self) {
        self.hold_expiring_for_test(MAX_HOLD);
    }

    /// As above, with an explicit lifetime so the watchdog can be
    /// tested without waiting it out.
    #[cfg(test)]
    pub(crate) fn hold_expiring_for_test(&self, within: Duration) {
        self.deadline_ms
            .store(self.now_ms() + within.as_millis() as u64, Ordering::Release);
        self.epoch.fetch_add(1, Ordering::AcqRel);
        self.want.store(true, Ordering::Release);
    }

    #[cfg(test)]
    pub(crate) fn want_release_for_test(&self) {
        self.want.store(false, Ordering::Release);
    }

    #[cfg(test)]
    pub(crate) fn is_held_for_test(&self) -> bool {
        self.held.load(Ordering::Acquire)
    }

    /// Drop any grab before the thread lets the devices go, so a
    /// shutdown mid-correction can't strand them.
    pub(crate) fn release_all<D: GateDevice>(&self, devices: &mut [D]) {
        self.want.store(false, Ordering::Release);
        self.service(devices);
    }
}
