//! Constants shared by the Windows listener, emitter and key gate.

/// Stamped into `dwExtraInfo` on every event we synthesise.
///
/// The hook's `LLKHF_INJECTED` flag answers "did *something* inject
/// this", which is not the question the key gate needs: another
/// automation tool's synthetic keys are injected too, and those we hold
/// back exactly like the user's. Only events carrying this marker are
/// *ours*, and only those may bypass the gate — otherwise a correction
/// would swallow its own backspaces.
///
/// The value is the same "POLT" the macOS emitter stamps into
/// `kCGEventSourceUserData`, for the same reason.
pub(super) const EMITTER_MARKER: usize = 0x504F_4C54;

/// `LLKHF_INJECTED` (0x10) | `LLKHF_LOWER_IL_INJECTED` (0x02).
pub(super) const LLKHF_INJECTED_ANY: u32 = 0x12;

/// Environment override for the key gate, read once at startup.
///
/// `POLTERTYPE_HOLD_KEYS=1` turns it on, `=0` off. The **default on
/// Windows is off**, which differs from Linux and is deliberate: the
/// evdev gate was developed and repeatedly broken on a machine its
/// author uses daily, while this one has never run on Windows at all.
/// A feature that can stop a keyboard working system-wide does not get
/// switched on for strangers by someone who cannot try it. Flip the
/// default once #7 has a confirmed report.
pub(super) const HOLD_KEYS_ENV: &str = "POLTERTYPE_HOLD_KEYS";
