//! Placement arithmetic and monitor picking. Pure except for the last
//! test, which is `#[ignore]`d because it needs a live X server.

use std::time::Duration;

use crate::enums::{PopupAnchor, WindowSpot};

use super::monitors::{self, MonitorRect, containing};
use super::show::place;

/// The reporter's desktop in issue #70: 1920×1080 at +0+120 beside
/// 1920×1200 at +1920+0, seam at x=1920, root 3840 wide.
const LEFT: MonitorRect = MonitorRect {
    x: 0,
    y: 120,
    w: 1920,
    h: 1080,
};
const RIGHT: MonitorRect = MonitorRect {
    x: 1920,
    y: 0,
    w: 1920,
    h: 1200,
};
const ROOT: MonitorRect = MonitorRect {
    x: 0,
    y: 0,
    w: 3840,
    h: 1200,
};

/// 420 px is about what four suggestions render to.
const W: u16 = 420;
const H: u16 = 160;

#[test]
fn a_second_monitor_takes_the_popup_off_the_seam() {
    let (x, _) = place(ROOT, W, H, &PopupAnchor::ScreenBottom);
    // What the report shows: centred on the whole root, straddling
    // x=1920 by half its width.
    assert!(
        (x as i32) < 1920 && x as i32 + W as i32 > 1920,
        "root-centred popup should cross the seam, was x={x}"
    );

    for mon in [LEFT, RIGHT] {
        let (x, y) = place(mon, W, H, &PopupAnchor::ScreenBottom);
        assert!(
            x as i32 >= mon.x && x as i32 + W as i32 <= mon.x + mon.w,
            "popup left its monitor horizontally: x={x} on {mon:?}"
        );
        assert!(
            y as i32 >= mon.y && y as i32 + H as i32 <= mon.y + mon.h,
            "popup left its monitor vertically: y={y} on {mon:?}"
        );
    }
}

#[test]
fn a_caret_beside_the_seam_keeps_the_popup_on_its_own_screen() {
    let caret = PopupAnchor::Point {
        x: 1900,
        y: 800,
        height: 20,
    };
    let (x, y) = place(LEFT, W, H, &caret);
    assert!(
        x as i32 + W as i32 <= LEFT.x + LEFT.w,
        "caret near the right edge pushed the popup onto the next monitor: x={x}"
    );
    assert!(y as i32 >= LEFT.y, "popup rose above its monitor: y={y}");
}

#[test]
fn a_window_anchor_is_pulled_back_inside_its_monitor() {
    // A window flush against the right monitor's right edge: centring
    // on it would hang the popup off the desktop.
    let win = PopupAnchor::WindowRect {
        x: 3500,
        y: 200,
        width: 340,
        height: 900,
        spot: WindowSpot::BottomCenter,
        margin: 96,
    };
    let (x, _) = place(RIGHT, W, H, &win);
    assert!(
        x as i32 >= RIGHT.x && x as i32 + W as i32 <= RIGHT.x + RIGHT.w,
        "window-anchored popup left its monitor: x={x}"
    );
}

#[test]
fn the_default_window_spot_is_where_the_window_anchor_always_was() {
    // Bottom-centre, 96 px up: the placement before the grid existed.
    let win = PopupAnchor::WindowRect {
        x: 200,
        y: 300,
        width: 1000,
        height: 600,
        spot: WindowSpot::default(),
        margin: 96,
    };
    let (x, y) = place(LEFT, W, H, &win);
    assert_eq!(x as i32, 200 + (1000 - W as i32) / 2);
    assert_eq!(y as i32, 300 + 600 - 96 - H as i32);
}

#[test]
fn a_corner_spot_sits_in_that_corner_of_the_window() {
    let at = |spot| {
        place(
            LEFT,
            W,
            H,
            &PopupAnchor::WindowRect {
                x: 200,
                y: 300,
                width: 1000,
                height: 600,
                spot,
                margin: 16,
            },
        )
    };
    assert_eq!(at(WindowSpot::TopLeft), (216, 316));
    assert_eq!(
        at(WindowSpot::BottomRight),
        ((1200 - 16 - W as i32) as i16, (900 - 16 - H as i32) as i16)
    );
}

#[test]
fn a_window_spot_off_the_monitor_is_pulled_back_onto_it() {
    // A window hanging off the left monitor's bottom-left corner: its
    // bottom-left spot lies off screen, the tooltip must not.
    let win = PopupAnchor::WindowRect {
        x: -300,
        y: 900,
        width: 800,
        height: 700,
        spot: WindowSpot::BottomLeft,
        margin: 16,
    };
    let (x, y) = place(LEFT, W, H, &win);
    assert_eq!(x as i32, LEFT.x);
    assert_eq!(y as i32, LEFT.y + LEFT.h - H as i32);
}

#[test]
fn one_screen_places_exactly_where_the_whole_root_did() {
    // The single-monitor case must not move: `for_anchor` hands the
    // one monitor straight through, and it equals the root.
    let single = MonitorRect {
        x: 0,
        y: 0,
        w: 1920,
        h: 1080,
    };
    let (x, y) = place(single, W, H, &PopupAnchor::ScreenBottom);
    assert_eq!(x as i32, (1920 - W as i32) / 2);
    assert_eq!(y as i32, 1080 - 96 - H as i32);
}

#[test]
fn a_point_belongs_to_the_screen_under_it_and_to_no_other() {
    let mons = [LEFT, RIGHT];
    assert_eq!(containing(&mons, 100, 500), Some(LEFT));
    assert_eq!(containing(&mons, 2000, 500), Some(RIGHT));
    // The left monitor starts 120 px down; that strip belongs to
    // neither, and the caller falls back rather than guessing.
    assert_eq!(containing(&mons, 100, 10), None);
}

/// The whole fix, against a real server, on the geometry that reported
/// it. Run it inside an X11 session that has two monitors — real, or
/// declared with `xrandr --setmonitor`, which is the same thing to
/// `GetMonitors` and therefore to us:
///
/// ```text
/// xrandr --setmonitor left  1920/500x1080/300+0+120  none
/// xrandr --setmonitor right 1920/500x1200/300+1920+0 none
/// cargo test -p poltertype-popup -- --ignored --nocapture
/// ```
#[test]
#[ignore = "needs a live X11 session with two monitors; see the doc comment"]
fn a_popup_with_nothing_to_anchor_to_lands_wholly_on_one_monitor() {
    use x11rb::connection::Connection;

    let (conn, screen_num) = x11rb::connect(None).expect("no X11 server");
    let screen = &conn.setup().roots[screen_num];
    let (root, root_rect) = (
        screen.root,
        MonitorRect {
            x: 0,
            y: 0,
            w: screen.width_in_pixels.into(),
            h: screen.height_in_pixels.into(),
        },
    );
    let mons = monitors::for_anchor(&conn, root, root_rect, &PopupAnchor::ScreenBottom);
    println!("chosen monitor: {mons:?} (root {root_rect:?})");

    // A desktop session has override-redirect windows of its own — a
    // panel's shadow, a screensaver's overlay — so ours is identified
    // as the one that was not there a moment ago.
    let before = mapped_windows(&conn, root);

    let (tx, _rx) = crossbeam_channel::unbounded();
    let popup = crate::create_popup(tx);
    assert_eq!(
        popup.backend_name(),
        "linux-x11-override-redirect",
        "this test measures the X11 backend"
    );
    popup.show(crate::types::PopupModel {
        generation: 1,
        original: "helllo".into(),
        entries: vec![crate::types::PopupEntry {
            text: "hello".into(),
            badge: None,
            is_action: false,
        }],
        accept_hint: Some("Ctrl+Shift".into()),
        timeout: Duration::from_secs(30),
        anchor: PopupAnchor::ScreenBottom,
        scale: 1.0,
        function_keys: false,
        arrows: false,
        selected: None,
    });
    std::thread::sleep(Duration::from_millis(1500));

    let after = mapped_windows(&conn, root);
    popup.hide();

    let all = monitors_of(&conn, root);
    let fresh: Vec<_> = after
        .iter()
        .filter(|(win, ..)| !before.iter().any(|(seen, ..)| seen == win))
        .collect();
    println!("monitors: {all:?}");
    println!("windows that appeared: {fresh:?}");
    let &(_, x, y, w, h) = *fresh.first().expect("the popup mapped no window");

    // The bug: centred in the root, which spans both monitors, the
    // popup crosses the seam between them.
    let root_centred = (root_rect.w - w) / 2;
    println!("popup at {x},{y} {w}x{h}; root-centred would be x={root_centred}");
    let home = all
        .iter()
        .find(|m| x >= m.x && y >= m.y && x + w <= m.x + m.w && y + h <= m.y + m.h)
        .unwrap_or_else(|| panic!("popup {x},{y} {w}x{h} lies on no single monitor of {all:?}"));
    println!("popup sits inside {home:?}");
    assert!(
        all.len() < 2
            || all
                .iter()
                .any(|m| root_centred < m.x + m.w && root_centred + w > m.x + m.w),
        "this layout does not reproduce the bug: a root-centred popup would not cross a seam, \
         so the test proves nothing. Declare two monitors that split the screen."
    );
}

/// Every mapped override-redirect child of the root, with its
/// geometry. Both halves of the before/after comparison.
fn mapped_windows(
    conn: &x11rb::rust_connection::RustConnection,
    root: u32,
) -> Vec<(u32, i32, i32, i32, i32)> {
    use x11rb::protocol::xproto::{ConnectionExt, MapState};

    let Ok(Ok(tree)) = conn.query_tree(root).map(|c| c.reply()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for win in tree.children {
        let Ok(Ok(attrs)) = conn.get_window_attributes(win).map(|c| c.reply()) else {
            continue;
        };
        if !attrs.override_redirect || attrs.map_state != MapState::VIEWABLE {
            continue;
        }
        let Ok(Ok(g)) = conn.get_geometry(win).map(|c| c.reply()) else {
            continue;
        };
        out.push((
            win,
            i32::from(g.x),
            i32::from(g.y),
            i32::from(g.width),
            i32::from(g.height),
        ));
    }
    out
}

/// The monitor list as the server reports it — the same call the
/// placement uses, spelled out here so the assertion above can say
/// which rectangle it landed in.
#[cfg(test)]
fn monitors_of(conn: &x11rb::rust_connection::RustConnection, root: u32) -> Vec<MonitorRect> {
    use x11rb::protocol::randr::ConnectionExt as _;
    let Ok(reply) = conn.randr_get_monitors(root, true).map(|c| c.reply()) else {
        return Vec::new();
    };
    match reply {
        Ok(reply) => reply
            .monitors
            .iter()
            .map(|m| MonitorRect {
                x: m.x.into(),
                y: m.y.into(),
                w: m.width.into(),
                h: m.height.into(),
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

// ─── Desktop scale (`Xft.dpi`) ───────────────────────────────────────

use super::dpi::scale_from_resources;

#[test]
fn xft_dpi_sets_the_desktop_scale() {
    // What cinnamon-settings-daemon writes at "User interface scaling:
    // Double", among the other Xft keys it manages.
    let cinnamon_2x = "Xft.antialias:\t1\nXft.dpi:\t192\nXft.hinting:\t1\n";
    assert_eq!(scale_from_resources(cinnamon_2x), 2.0);
    assert_eq!(scale_from_resources("Xft.dpi: 144\n"), 1.5);
    assert_eq!(scale_from_resources("Xft.dpi:\t120"), 1.25);
}

#[test]
fn no_or_nonsense_xft_dpi_draws_as_designed() {
    assert_eq!(scale_from_resources(""), 1.0);
    assert_eq!(scale_from_resources("Xcursor.size:\t48\n"), 1.0);
    assert_eq!(scale_from_resources("Xft.dpi:\tlarge\n"), 1.0);
    assert_eq!(scale_from_resources("Xft.dpi:\t0\n"), 1.0);
    // A desktop set below standard does not shrink the tooltip, and a
    // typo the size of a wall does not grow it past a screen.
    assert_eq!(scale_from_resources("Xft.dpi:\t72\n"), 1.0);
    assert_eq!(scale_from_resources("Xft.dpi:\t9600\n"), 4.0);
    // A different key that merely ends in the same name is not it.
    assert_eq!(scale_from_resources("MyApp.Xft.dpi:\t192\n"), 1.0);
}
