//! Constants shared by the Windows listener, emitter and key gate.

/// Stamped into `dwExtraInfo` on every event we synthesise.
///
/// `LLKHF_INJECTED` answers "did *something* inject this", which is not
/// the question the key gate needs: another automation tool's synthetic
/// keys are injected too, and those we hold back exactly like the
/// user's. Only events carrying this marker are *ours* and may bypass
/// the gate — otherwise a correction would swallow its own backspaces.
///
/// Same "POLT" the macOS emitter stamps into
/// `kCGEventSourceUserData`, for the same reason.
pub(super) const EMITTER_MARKER: usize = 0x504F_4C54;

/// Environment override for the key gate, read once at startup:
/// `POLTERTYPE_HOLD_KEYS=1` on, `=0` off.
///
/// **Default off on Windows, as on macOS, and for the same reason: not
/// fear, but latency.** The gate has run on real Windows hardware
/// (issue #7) and no keyboard wedge was ever observed; what it costs is
/// the ~75-100 ms of withheld keystrokes after every correction, which
/// reads as the caret lagging behind your typing. Switch it on if you
/// type fast enough to hit the race; `docs/PERMISSIONS.md` states the
/// trade.
pub(super) const HOLD_KEYS_ENV: &str = "POLTERTYPE_HOLD_KEYS";
