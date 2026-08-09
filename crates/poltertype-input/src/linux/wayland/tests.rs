//! Unit tests for the evdev backend's decision-making: the key gate
//! and the rescan bookkeeping.
//!
//! Neither can be exercised against real hardware from a test, so both
//! are driven through the seams they were given for it — [`GateDevice`]
//! for the gate, a pure path-diff for the rescan. Every case here is a
//! regression that cost real text on a real machine.

use super::*;

use std::collections::HashSet;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// A device the gate can drive without any hardware. Counts the two
/// syscalls, and can pretend to be busy (an input remapper already
/// holds it), a mouse, or idle.
struct FakeDevice {
    name: &'static str,
    gate: GateState,
    busy: bool,
    grabs: usize,
    ungrabs: usize,
}

impl FakeDevice {
    /// A keyboard in active use — the common case.
    fn keyboard(name: &'static str) -> Self {
        Self {
            name,
            gate: GateState {
                is_keyboard: true,
                last_event: Some(Instant::now()),
                ..GateState::default()
            },
            busy: false,
            grabs: 0,
            ungrabs: 0,
        }
    }

    fn ours(mut self) -> Self {
        self.gate.is_ours = true;
        self
    }

    fn mouse(mut self) -> Self {
        self.gate.is_keyboard = false;
        self
    }

    /// Silent for longer than [`RECENT_USE_WINDOW`].
    fn idle(mut self) -> Self {
        self.gate.last_event = Some(Instant::now() - RECENT_USE_WINDOW - Duration::from_secs(1));
        self
    }

    /// Held exclusively by someone else — what keyd does to every
    /// physical keyboard on the author's machine.
    fn busy(mut self) -> Self {
        self.busy = true;
        self
    }
}

impl GateDevice for FakeDevice {
    fn grab(&mut self) -> io::Result<()> {
        self.grabs += 1;
        if self.busy {
            return Err(io::Error::from_raw_os_error(libc::EBUSY));
        }
        Ok(())
    }

    fn ungrab(&mut self) -> io::Result<()> {
        self.ungrabs += 1;
        Ok(())
    }

    fn state(&self) -> &GateState {
        &self.gate
    }

    fn state_mut(&mut self) -> &mut GateState {
        &mut self.gate
    }

    fn label(&self) -> String {
        self.name.to_owned()
    }
}

/// An available gate. The real one decides availability by probing the
/// emitter device, which a test has no way to arrange.
fn ready_gate() -> EvdevGate {
    let g = EvdevGate::new();
    g.mark_available_for_test();
    g
}

/// Stand in for the device thread: one poll of the read loop.
fn poll(gate: &EvdevGate, devices: &mut [FakeDevice]) {
    gate.service(devices);
}

#[test]
fn holds_only_the_keyboard_in_use() {
    let gate = ready_gate();
    let mut devices = [
        FakeDevice::keyboard("active-keyboard"),
        FakeDevice::keyboard("mouse").mouse(),
        FakeDevice::keyboard("unused-keyboard").idle(),
        FakeDevice::keyboard("our-emitter").ours(),
    ];

    gate.hold_for_test();
    poll(&gate, &mut devices);

    assert!(devices[0].gate.grabbed, "the keyboard in use must be held");
    assert_eq!(
        devices[1].grabs, 0,
        "a mouse delivers no keystrokes to race"
    );
    assert_eq!(
        devices[2].grabs, 0,
        "an idle keyboard is not worth the release cost"
    );
    assert!(
        !devices[3].gate.grabbed,
        "our own emitter must never stay grabbed"
    );
    assert_eq!(
        devices[3].grabs, devices[3].ungrabs,
        "the proxy re-verification must give the emitter straight back"
    );
}

/// Regression for the 2026-07-31 session lockup: the startup probe
/// races an input remapper's asynchronous grab of our freshly created
/// emitter. If the remapper wins after the probe said "unproxied", a
/// hold would funnel every input path into this process — so the
/// per-hold re-verification must catch the EBUSY, refuse the hold and
/// turn the gate off for the rest of the run.
#[test]
fn a_hold_is_refused_when_our_emitter_became_proxied() {
    let gate = ready_gate();
    let mut devices = [
        FakeDevice::keyboard("active-keyboard"),
        FakeDevice::keyboard("our-emitter").ours().busy(),
    ];

    gate.hold_for_test();
    poll(&gate, &mut devices);

    assert!(
        !gate.is_held_for_test(),
        "the hold must be refused outright"
    );
    assert_eq!(
        devices[0].grabs, 0,
        "no user keyboard may be grabbed once the emitter is proxied"
    );
    assert!(!gate.available(), "the gate must stay off until restart");

    // …and a later hold must not even reach the devices.
    gate.hold_for_test();
    poll(&gate, &mut devices);
    assert_eq!(devices[0].grabs, 0);
}

#[test]
fn a_busy_device_is_tried_once_per_hold_not_once_per_poll() {
    let gate = ready_gate();
    let mut devices = [FakeDevice::keyboard("claimed-by-keyd").busy()];

    gate.hold_for_test();
    for _ in 0..10 {
        poll(&gate, &mut devices);
    }
    assert_eq!(
        devices[0].grabs, 1,
        "retrying an EBUSY device every poll spends the read loop's budget on failing ioctls"
    );

    // A fresh hold gets a fresh attempt — the remapper may have let go.
    gate.hold_for_test();
    poll(&gate, &mut devices);
    assert_eq!(devices[0].grabs, 2);
}

#[test]
fn nothing_grabbable_means_the_hold_reports_failure() {
    let gate = ready_gate();
    let mut devices = [FakeDevice::keyboard("claimed").busy()];

    gate.hold_for_test();
    poll(&gate, &mut devices);

    assert!(
        !gate.is_held_for_test(),
        "with nothing held the correction must know to protect itself the old way"
    );
}

#[test]
fn release_gives_every_device_back() {
    let gate = ready_gate();
    let mut devices = [FakeDevice::keyboard("kbd-a"), FakeDevice::keyboard("kbd-b")];

    gate.hold_for_test();
    poll(&gate, &mut devices);
    assert!(devices.iter().all(|d| d.gate.grabbed));

    gate.want_release_for_test();
    poll(&gate, &mut devices);

    assert!(devices.iter().all(|d| !d.gate.grabbed));
    assert!(devices.iter().all(|d| d.ungrabs == 1));
    assert!(!gate.is_held_for_test());
}

#[test]
fn a_keyboard_appearing_mid_hold_is_covered_too() {
    let gate = ready_gate();
    let mut devices = vec![FakeDevice::keyboard("kbd-a")];

    gate.hold_for_test();
    poll(&gate, &mut devices);

    devices.push(FakeDevice::keyboard("hotplugged"));
    poll(&gate, &mut devices);

    assert!(
        devices[1].gate.grabbed,
        "a keyboard the rescan picks up mid-hold still delivers keystrokes into our burst"
    );
}

#[test]
fn the_watchdog_releases_a_hold_the_engine_forgot() {
    let gate = ready_gate();
    let mut devices = [FakeDevice::keyboard("kbd")];

    gate.hold_expiring_for_test(Duration::ZERO);
    poll(&gate, &mut devices);

    assert!(
        !devices[0].gate.grabbed && !gate.is_held_for_test(),
        "a hung correction must never be able to leave the keyboard dead"
    );
}

#[test]
fn shutdown_never_leaves_a_device_grabbed() {
    let gate = ready_gate();
    let mut devices = [FakeDevice::keyboard("kbd")];

    gate.hold_for_test();
    poll(&gate, &mut devices);
    gate.release_all(&mut devices);

    assert!(!devices[0].gate.grabbed);
    assert_eq!(devices[0].ungrabs, 1);
}

#[test]
fn an_unavailable_gate_touches_nothing() {
    let gate = EvdevGate::new(); // availability never probed
    let mut devices = [FakeDevice::keyboard("kbd")];

    assert!(
        !gate.hold(),
        "an unavailable gate must report it cannot hold"
    );
    poll(&gate, &mut devices);
    assert_eq!(devices[0].grabs, 0);
}

// ─── Rescan bookkeeping ──────────────────────────────────────────────

fn paths(names: &[&str]) -> HashSet<PathBuf> {
    names.iter().map(PathBuf::from).collect()
}

#[test]
fn rescan_opens_only_genuinely_new_nodes() {
    let known = paths(&["/dev/input/event0", "/dev/input/event1"]);
    let present = paths(&[
        "/dev/input/event0",
        "/dev/input/event1",
        "/dev/input/event2",
    ]);

    let (fresh, forgotten) = plan_rescan(&present, &known);

    assert_eq!(fresh, vec![PathBuf::from("/dev/input/event2")]);
    assert!(
        forgotten.is_empty(),
        "re-judging a node costs an open plus a capability read, and most are sound cards"
    );
}

#[test]
fn rescan_forgets_nodes_that_disappeared() {
    let known = paths(&["/dev/input/event0", "/dev/input/event9"]);
    let present = paths(&["/dev/input/event0"]);

    let (fresh, forgotten) = plan_rescan(&present, &known);

    assert!(fresh.is_empty());
    assert_eq!(forgotten, vec![PathBuf::from("/dev/input/event9")]);
}

#[test]
fn a_device_replugged_onto_the_same_node_is_seen_again() {
    // The exact sequence that silently lost a keyboard: unplug (node
    // gone), replug (same node number, different device).
    let mut known = paths(&["/dev/input/event5"]);

    let (_, forgotten) = plan_rescan(&paths(&[]), &known);
    for p in forgotten {
        known.remove(&p);
    }

    let (fresh, _) = plan_rescan(&paths(&["/dev/input/event5"]), &known);
    assert_eq!(
        fresh,
        vec![PathBuf::from("/dev/input/event5")],
        "the node is reused, so the device behind it has to be judged afresh"
    );
}
