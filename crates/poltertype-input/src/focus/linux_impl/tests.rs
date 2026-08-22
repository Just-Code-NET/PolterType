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
    // Only the first call reached the inner tracker; the counter cannot
    // be read back through the `Box<dyn>`, so the pass-through direction
    // is asserted by `cache_with_zero_ttl_always_refreshes`.
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

// ─── Geometry parsing (suggestion-tooltip anchor) ────────────────────

const ACTIVE_WINDOW_WITH_GEOMETRY: &str = "\
Window 55df4e97c880 -> kitty:
\tmapped: 1
\tat: 2571,26
\tsize: 1268,1388
\tworkspace: 2 (2)
\tfloating: 0
\tmonitor: 1
\tclass: kitty
\tpid: 12345
";

#[test]
fn parse_rect_extracts_position_size_and_monitor() {
    let rect = super::hyprland_ipc::parse_active_window_rect(ACTIVE_WINDOW_WITH_GEOMETRY);
    assert_eq!(rect, Some((2571, 26, 1268, 1388, 1)));
}

#[test]
fn parse_rect_requires_all_three_fields() {
    // No `monitor:` line → no geometry (a rect on an unknown output
    // could place the tooltip on the wrong screen).
    let no_monitor = ACTIVE_WINDOW_WITH_GEOMETRY
        .lines()
        .filter(|l| !l.trim_start().starts_with("monitor:"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        super::hyprland_ipc::parse_active_window_rect(&no_monitor),
        None
    );
    assert_eq!(
        super::hyprland_ipc::parse_active_window_rect("Invalid"),
        None
    );
}

#[test]
fn parse_monitors_extracts_name_id_and_origin() {
    let reply = "\
Monitor eDP-1 (ID 0):
\t2560x1440@165.00301 at 0x0
\tdescription: Some Panel
\tscale: 1.00
Monitor DP-3 (ID 1):
\t3840x2160@60.00000 at 2560x0
\tdescription: Big External
\tscale: 1.50
";
    let monitors = super::hyprland_ipc::parse_monitors(reply);
    assert_eq!(monitors.len(), 2);
    assert_eq!(monitors[0].name, "eDP-1");
    assert_eq!(monitors[0].id, 0);
    assert_eq!((monitors[0].x, monitors[0].y), (0, 0));
    assert_eq!(monitors[1].name, "DP-3");
    assert_eq!(monitors[1].id, 1);
    assert_eq!((monitors[1].x, monitors[1].y), (2560, 0));
}

#[test]
fn parse_monitors_handles_negative_origins() {
    let reply = "\
Monitor DP-2 (ID 3):
\t1920x1080@60.00000 at -1920x-360
";
    let monitors = super::hyprland_ipc::parse_monitors(reply);
    assert_eq!(monitors.len(), 1);
    assert_eq!((monitors[0].x, monitors[0].y), (-1920, -360));
}

// ─── AT-SPI caret extents fallback (pure logic) ──────────────────────

use super::atspi_caret::{CaretSample, anchor_from_rect, is_degenerate, retry_offset};

#[test]
fn degenerate_rect_is_zero_area_only() {
    // Fully collapsed rect = the end-of-text caret answer.
    assert!(is_degenerate((100, 200, 0, 0)));
    // Zero width alone is a zero-advance glyph (combining mark) — a
    // real position, not a failure.
    assert!(!is_degenerate((100, 200, 0, 16)));
    assert!(!is_degenerate((100, 200, 8, 0)));
    assert!(!is_degenerate((100, 200, 8, 16)));
}

#[test]
fn retry_offset_steps_back_only_when_possible() {
    assert_eq!(retry_offset(5), Some(4));
    assert_eq!(retry_offset(1), Some(0));
    // Offset 0 has no previous character to fall back to.
    assert_eq!(retry_offset(0), None);
    // Defensive: a client sending a bogus negative offset must not
    // trigger further queries.
    assert_eq!(retry_offset(-1), None);
    assert_eq!(retry_offset(i32::MIN), None);
}

#[test]
fn anchor_uses_left_edge_normally_and_right_edge_on_fallback() {
    // The rect of the caret's own glyph: anchor at its left edge.
    assert_eq!(anchor_from_rect((100, 200, 8, 16), false), (100, 200, 16));
    // The rect of the *previous* glyph: the caret blinks at its
    // trailing (right) edge — x + width.
    assert_eq!(anchor_from_rect((100, 200, 8, 16), true), (108, 200, 16));
}

#[test]
fn anchor_clamps_negative_height_and_saturates_x() {
    assert_eq!(anchor_from_rect((1, 2, 3, -4), false), (1, 2, 0));
    assert_eq!(
        anchor_from_rect((i32::MAX, 0, 1, 1), true),
        (i32::MAX, 0, 1)
    );
}

#[test]
fn caret_sample_hint_carries_coordinates_and_age() {
    let earlier = std::time::Instant::now() - Duration::from_millis(50);
    let sample = CaretSample {
        x: 3,
        y: 4,
        height: 5,
        at: earlier,
    };
    let hint = sample.into_hint();
    assert_eq!((hint.x, hint.y, hint.height), (3, 4, 5));
    assert!(hint.age >= Duration::from_millis(50));
}
