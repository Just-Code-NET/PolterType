use std::sync::Mutex;

use super::*;

/// Every test here drives the gate through the same process-global
/// environment variable and the harness runs them on separate
/// threads: without serialising, one test's `remove_var` lands
/// between another's `set_var` and its `MacosGate::new()` — an
/// intermittent failure that only ever appears on the macOS CI job.
///
/// Poisoning is stepped over deliberately: one failing test should
/// not become a cascade of panics.
static ENV: Mutex<()> = Mutex::new(());

#[test]
fn unavailable_until_the_tap_reports_running() {
    let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
    // Enabled via env so the test is independent of the default.
    unsafe { std::env::set_var(HOLD_KEYS_ENV, "1") };
    let g = MacosGate::new(false);
    assert!(!g.available(), "no tap yet — must not claim to hold");
    assert!(!g.hold(), "hold without a tap reports unheld");
    g.set_tap_running(true);
    assert!(g.available());
    assert!(g.hold());
    g.set_tap_running(false);
    assert!(!g.available(), "tap gone — holds unavailable again");
    unsafe { std::env::remove_var(HOLD_KEYS_ENV) };
}

#[test]
fn env_zero_disables_even_with_a_running_tap() {
    let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::set_var(HOLD_KEYS_ENV, "0") };
    let g = MacosGate::new(true);
    g.set_tap_running(true);
    assert!(!g.available());
    assert!(!g.hold());
    unsafe { std::env::remove_var(HOLD_KEYS_ENV) };
}

#[test]
fn config_decides_when_env_is_unset() {
    let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::remove_var(HOLD_KEYS_ENV) };
    let on = MacosGate::new(true);
    on.set_tap_running(true);
    assert!(on.available(), "config on, env unset — gate holds");
    let off = MacosGate::new(false);
    off.set_tap_running(true);
    assert!(!off.available(), "config off, env unset — gate stays out");
}
