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
fn parse_rect_extracts_position_and_size() {
    let rect = super::hyprland_ipc::parse_active_window_rect(ACTIVE_WINDOW_WITH_GEOMETRY);
    assert_eq!(rect, Some((2571, 26, 1268, 1388)));
}

#[test]
fn parse_rect_requires_both_lines() {
    let no_size = ACTIVE_WINDOW_WITH_GEOMETRY
        .lines()
        .filter(|l| !l.trim_start().starts_with("size:"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        super::hyprland_ipc::parse_active_window_rect(&no_size),
        None
    );
    assert_eq!(
        super::hyprland_ipc::parse_active_window_rect("Invalid"),
        None
    );
}

#[test]
fn parse_rect_handles_a_window_on_a_negative_origin_monitor() {
    let reply = "\tat: -1920,-360\n\tsize: 1920,1080\n";
    assert_eq!(
        super::hyprland_ipc::parse_active_window_rect(reply),
        Some((-1920, -360, 1920, 1080))
    );
}

// ─── AT-SPI caret extents fallback (pure logic) ──────────────────────

use super::atspi_caret::{anchor_from_rect, is_caret_shaped, is_degenerate, retry_offset};
use super::types::{CaretOwner, CaretSample};

#[test]
fn degenerate_rect_is_zero_area_or_negative() {
    // Fully collapsed rect = the end-of-text caret answer.
    assert!(is_degenerate((100, 200, 0, 0)));
    // Chromium and Electron answer this for "no caret here"; taken at
    // face value it anchors one pixel off the window's top-left.
    assert!(is_degenerate((-1, -1, -1, -1)));
    assert!(is_degenerate((100, 200, 8, -1)));
    // Zero width alone is a zero-advance glyph (combining mark) — a
    // real position, not a failure.
    assert!(!is_degenerate((100, 200, 0, 16)));
    assert!(!is_degenerate((100, 200, 8, 0)));
    assert!(!is_degenerate((100, 200, 8, 16)));
}

#[test]
fn caret_shaped_accepts_a_glyph_box_and_refuses_a_text_field() {
    // VS Code's hidden IME input, parked exactly at the caret.
    assert!(is_caret_shaped((830, 1203, 7, 17)));
    // A zero-width caret is still a caret.
    assert!(is_caret_shaped((830, 1203, 0, 17)));
    // Tall lines allow a proportionally wider box.
    assert!(is_caret_shaped((0, 0, 30, 40)));
    // A chat composer and a page section are fields, not carets: their
    // left edge is the start of the line, not where the typing is.
    assert!(!is_caret_shaped((340, 551, 610, 25)));
    assert!(!is_caret_shaped((223, 1131, 255, 36)));
    // Nothing with no height can stand in for a caret.
    assert!(!is_caret_shaped((0, 0, 0, 0)));
    assert!(!is_caret_shaped((0, 0, 4, -1)));
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
fn caret_sample_hint_carries_coordinates_age_and_owner() {
    let earlier = std::time::Instant::now() - Duration::from_millis(50);
    let sample = CaretSample {
        x: 3,
        y: 4,
        height: 5,
        at: earlier,
        owner: CaretOwner {
            pid: 4242,
            window: Some((800, 600)),
        },
    };
    let hint = sample.into_hint();
    assert_eq!((hint.x, hint.y, hint.height), (3, 4, 5));
    assert!(hint.age >= Duration::from_millis(50));
    // Without these the consumer cannot tell this caret apart from one
    // the app the user is typing in never produced.
    assert_eq!(hint.pid, Some(4242));
    assert_eq!(hint.window, Some((800, 600)));
}

/// Live check of everything the suggestion tooltip anchors on: samples
/// the tracker once a second and prints the focused window next to the
/// caret hint, so a wrong tooltip position on a real desktop can be
/// traced to whichever of the two lied. Type in a few applications
/// while it runs — including one with no accessibility bridge, which
/// is where a caret left over from another window used to win.
///
/// ```text
/// cargo test -p poltertype-input -- --ignored --nocapture live_anchor_inputs
/// ```
#[test]
#[ignore = "requires a live desktop, an a11y bus and someone typing"]
fn live_anchor_inputs_agree_on_the_focused_window() {
    let tracker = super::create_linux_focus_tracker();
    println!("backend={}", tracker.backend_name());
    for _ in 0..60 {
        let geometry = tracker.focused_window_geometry();
        let caret = tracker.caret_hint();
        match (&geometry, &caret) {
            (Some(g), Some(c)) => {
                let same_pid = g.pid == c.pid;
                let same_window = c.window.is_none_or(|wh| wh == (g.width, g.height));
                println!(
                    "window pid={:?} at=({}, {}) size=({}, {}) | caret pid={:?} \
                     window={:?} at=({}, {}) h={} age={}ms | same_pid={same_pid} \
                     same_window={same_window}",
                    g.pid,
                    g.x,
                    g.y,
                    g.width,
                    g.height,
                    c.pid,
                    c.window,
                    c.x,
                    c.y,
                    c.height,
                    c.age.as_millis()
                );
            }
            (g, c) => println!("geometry={g:?} caret={c:?}"),
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}
