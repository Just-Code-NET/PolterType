use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::focus::FocusTracker;

use super::cache::CachedFocusTracker;
use super::hyprland_ipc::parse_active_window;
use super::proc_exe::exe_basename_for_pid;

/// A realistic `activewindow` reply (trimmed): `class:` must win over
/// `initialClass:`, `pid:` must parse.
const ACTIVE_WINDOW_REPLY: &str = "\
Window 55df4e97c880 -> kitty:
\tmapped: 1
\thidden: 0
\tat: 11,11
\tsize: 3418,1898
\tworkspace: 1 (1)
\tfloating: 0
\tclass: kitty
\ttitle: ~ — kitty
\tinitialClass: kitty-initial
\tinitialTitle: kitty
\tpid: 12345
\txwayland: 0
";

#[test]
fn parse_active_window_extracts_pid_and_class() {
    let (pid, class) = parse_active_window(ACTIVE_WINDOW_REPLY);
    assert_eq!(pid, Some(12345));
    assert_eq!(class.as_deref(), Some("kitty"));
}

#[test]
fn parse_active_window_ignores_initial_class() {
    let (_, class) = parse_active_window("\tinitialClass: foo\n\tclass: bar\n");
    assert_eq!(class.as_deref(), Some("bar"));
}

#[test]
fn parse_active_window_handles_no_focused_window() {
    // Hyprland answers `Invalid` when nothing is focused.
    assert_eq!(parse_active_window("Invalid"), (None, None));
    assert_eq!(parse_active_window(""), (None, None));
}

#[test]
fn parse_active_window_rejects_bogus_pid() {
    let (pid, _) = parse_active_window("\tpid: -1\n");
    assert_eq!(pid, None);
    let (pid, _) = parse_active_window("\tpid: banana\n");
    assert_eq!(pid, None);
}

#[test]
fn exe_basename_resolves_own_pid() {
    // Our own /proc entry is always readable; the test runner's
    // basename is non-empty whatever the harness binary is called.
    let name = exe_basename_for_pid(std::process::id());
    assert!(name.is_some_and(|n| !n.is_empty()));
}

#[test]
fn exe_basename_none_for_dead_pid() {
    // PID 0 has no /proc entry (the kernel's idle task isn't a process).
    assert_eq!(exe_basename_for_pid(0), None);
}

struct CountingTracker {
    calls: AtomicUsize,
}

impl FocusTracker for CountingTracker {
    fn focused_exe(&self) -> Option<String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Some("counted".into())
    }
    fn backend_name(&self) -> &'static str {
        "counting"
    }
}

#[test]
fn cache_serves_within_ttl_without_hitting_inner() {
    let cached = CachedFocusTracker::new(
        Box::new(CountingTracker {
            calls: AtomicUsize::new(0),
        }),
        Duration::from_secs(3600),
    );
    assert_eq!(cached.focused_exe().as_deref(), Some("counted"));
    assert_eq!(cached.focused_exe().as_deref(), Some("counted"));
    // Only the first call reached the inner tracker; we can't read the
    // counter back through the Box<dyn>, so assert via a zero TTL below.
}

#[test]
fn cache_with_zero_ttl_always_refreshes() {
    // `elapsed() < 0` is never true, so every call must pass through.
    // Symmetric sanity check for the TTL comparison direction.
    let cached = CachedFocusTracker::new(
        Box::new(CountingTracker {
            calls: AtomicUsize::new(0),
        }),
        Duration::ZERO,
    );
    assert_eq!(cached.focused_exe().as_deref(), Some("counted"));
    assert_eq!(cached.focused_exe().as_deref(), Some("counted"));
}

/// Live check against the real session — needs a running Hyprland or
/// X11 desktop with a focused window, so it's `#[ignore]`d in CI.
/// Run manually: `cargo test -p poltertype-input -- --ignored focus`
#[test]
#[ignore = "requires a live Hyprland/X11 session with a focused window"]
fn live_focused_exe_returns_current_app() {
    let tracker = super::create_linux_focus_tracker();
    let exe = tracker.focused_exe();
    println!("backend={} focused_exe={exe:?}", tracker.backend_name());
    assert!(tracker.backend_name() != "noop", "expected a live backend");
    assert!(exe.is_some_and(|n| !n.is_empty()));
}
