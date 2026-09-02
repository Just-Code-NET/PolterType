use poltertype_types::LayoutId;

use super::consts::*;
use super::region::region_of;
use super::table;
use crate::icon_render::{H, PanelPolarity, W};

/// How many countries the table draws. A number to be changed on
/// purpose: it is the only thing standing between "I added Mexico"
/// and "I added a second Italy".
const DRAWN_COUNTRIES: usize = 39;

fn blank() -> Vec<u8> {
    vec![0u8; (W * H * 4) as usize]
}

/// Every two-letter code there is, so the table is measured by what it
/// answers rather than by a list kept beside it.
fn every_drawing() -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for a in b'A'..=b'Z' {
        for b in b'A'..=b'Z' {
            let region = String::from_utf8_lossy(&[a, b]).into_owned();
            let mut buf = blank();
            if table::draw(&mut buf, &region) {
                out.push((region, buf));
            }
        }
    }
    out
}

fn px(buf: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * W + x) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
}

#[test]
fn region_of_reads_the_country_not_the_language() {
    assert_eq!(region_of(&LayoutId::from("en-US")).as_deref(), Some("US"));
    assert_eq!(region_of(&LayoutId::from("en-GB")).as_deref(), Some("GB"));
    assert_eq!(region_of(&LayoutId::from("uk-UA")).as_deref(), Some("UA"));
    assert_eq!(
        region_of(&LayoutId::from("kk-Cyrl-KZ")).as_deref(),
        Some("KZ")
    );
}

/// Four ways a layout can name no country at all. Each has to answer
/// `None` rather than a flag: the opaque ids are what Windows and
/// macOS fall back to, `ar` names a language spoken under twenty
/// flags, and `419` is half a continent.
#[test]
fn a_layout_that_names_no_country_has_no_flag() {
    for id in ["hkl:00000409", "com.apple.keylayout.US", "ar", "es-419"] {
        assert_eq!(region_of(&LayoutId::from(id)), None, "{id}");
    }
}

#[test]
fn every_flag_in_the_table_is_its_own_drawing() {
    let drawings = every_drawing();
    assert_eq!(drawings.len(), DRAWN_COUNTRIES);
    for (i, (ra, a)) in drawings.iter().enumerate() {
        for (rb, b) in &drawings[i + 1..] {
            assert_ne!(a, b, "{ra} and {rb} are drawn the same");
        }
    }
}

/// The table only holds flags that can be told apart at a panel's
/// size. These four are white-blue-red bands plus an emblem too small
/// to draw — Russia with a lie on it — so they keep the letters.
#[test]
fn a_flag_that_would_be_a_lie_is_left_undrawn() {
    for region in ["SK", "SI", "RS", "HR", "MX", "ZZ"] {
        let mut buf = blank();
        assert!(!table::draw(&mut buf, region), "{region}");
        assert_eq!(buf, blank(), "{region} drew something anyway");
    }
}

/// The drawing owns its box and nothing else: the two design units
/// above it are where the waiting badge is drawn, and a flag that
/// spilled into them would be the icon's whole top edge.
#[test]
fn the_drawing_stays_inside_its_box() {
    let mut buf = blank();
    assert!(table::draw(&mut buf, "JP"));
    for y in 0..FY {
        for x in 0..W {
            assert_eq!(px(&buf, x, y), [0, 0, 0, 0], "spilled at {x},{y}");
        }
    }
}

/// A flag has no tile to grey, so pausing has to flatten the flag
/// itself — otherwise Japan paused and Japan running are the same
/// picture with two bars added.
#[test]
fn a_paused_flag_is_grey() {
    let running = super::render(&LayoutId::from("uk-UA"), false, PanelPolarity::Dark);
    let paused = super::render(&LayoutId::from("uk-UA"), true, PanelPolarity::Dark);
    assert!(running.is_some(), "uk-UA has a flag");
    assert_ne!(running, paused);
    if let Some(buf) = paused {
        for y in FY + EDGE..FY + FH - EDGE {
            for x in EDGE..FW - EDGE {
                let [r, g, b, _] = px(&buf, x, y);
                assert!(r == g && g == b, "colour left at {x},{y}");
            }
        }
    }
}

#[test]
fn the_edge_takes_the_panel_it_is_drawn_on() {
    for (polarity, expected) in [
        (PanelPolarity::Dark, EDGE_ON_DARK),
        (PanelPolarity::Light, EDGE_ON_LIGHT),
    ] {
        let buf = super::render(&LayoutId::from("uk-UA"), false, polarity);
        assert_eq!(buf.as_deref().map(|b| px(b, 0, FY)), Some(expected));
    }
}
