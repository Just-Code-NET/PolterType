use poltertype_layout::LayoutId;

use super::*;

#[test]
fn short_code_takes_first_two_letters() {
    assert_eq!(layout_short_code(&LayoutId::from("en-US")), "EN");
    assert_eq!(layout_short_code(&LayoutId::from("uk-UA")), "UK");
    assert_eq!(layout_short_code(&LayoutId::from("kk-Cyrl-KZ")), "KK");
}

#[test]
fn short_code_falls_back_for_opaque_id() {
    assert_eq!(layout_short_code(&LayoutId::from("hkl:00000409")), "??");
}

#[test]
fn render_produces_expected_buffer_size() {
    let buf = render(b"EN", [0x4F, 0x9D, 0xFF, 0xFF]);
    assert_eq!(buf.len(), (W * H * 4) as usize);
}

#[test]
fn icon_is_buildable_for_known_layouts() {
    assert!(for_layout(&LayoutId::from("en-US"), false, false).is_ok());
    assert!(for_layout(&LayoutId::from("uk-UA"), false, true).is_ok());
}

#[test]
fn paused_icon_differs_from_active_icon() {
    // Smoke test: the paused variant should produce a different
    // pixel buffer than the active variant for the same layout.
    // (We compare the underlying render() output to avoid
    // depending on Icon equality.)
    let normal = render(b"EN", color_for(&LayoutId::from("en-US")));
    let mut paused = render(b"EN", PAUSED_BG);
    draw_pause_indicator(&mut paused);
    assert_ne!(normal, paused);
}

#[test]
fn the_waiting_badge_leaves_the_layout_code_alone() {
    // The icon's job is to name the layout; the badge is a guest on it.
    // Every pixel the two glyphs occupy must read the same with the mark
    // as without, or a "UK" with drafts waiting is a different word.
    let plain = render(b"UK", [0x4F, 0x9D, 0xFF, 0xFF]);
    let mut marked = plain.clone();
    draw_waiting_badge(&mut marked);
    let px = |buf: &[u8], x: u32, y: u32| {
        let i = ((y * W + x) * 4) as usize;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    };
    for y in 5..11 {
        for x in 3..12 {
            assert_eq!(
                px(&plain, x, y),
                px(&marked, x, y),
                "the badge touched the glyphs at {x},{y}"
            );
        }
    }
    // …and it does mark it somewhere, or it would be a no-op that passes.
    assert_ne!(plain, marked);
}
