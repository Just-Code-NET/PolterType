use poltertype_core::settings::TrayIconStyle;
use poltertype_layout::LayoutId;

use super::*;

/// One design unit, read back out of the scaled buffer.
fn unit(buf: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = (((y * SCALE) * W + x * SCALE) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
}

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

/// The icon a panel is handed is bigger than the grid it is drawn on,
/// so a host scaling it up has something to scale (issue #54) — and one
/// design unit is a whole block of identical pixels, not a smear.
#[test]
fn the_icon_is_drawn_larger_than_its_design_grid() {
    const { assert!(W > UNITS && W == UNITS * SCALE) };
    let buf = render(b"EN", [0x4F, 0x9D, 0xFF, 0xFF]);
    let corner = unit(&buf, 0, 0);
    for dy in 0..SCALE {
        for dx in 0..SCALE {
            let i = ((dy * W + dx) * 4) as usize;
            assert_eq!(
                [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]],
                corner,
                "a design unit must be one solid block"
            );
        }
    }
}

#[test]
fn icon_is_buildable_for_known_layouts() {
    for style in [
        TrayIconStyle::Color,
        TrayIconStyle::Mono,
        TrayIconStyle::Hidden,
    ] {
        assert!(
            for_layout(
                &LayoutId::from("en-US"),
                false,
                false,
                style,
                PanelPolarity::Dark
            )
            .is_ok()
        );
    }
    assert!(
        for_layout(
            &LayoutId::from("uk-UA"),
            true,
            true,
            TrayIconStyle::Mono,
            PanelPolarity::Light
        )
        .is_ok()
    );
}

/// `mono` is the letters and nothing else: a filled tile is what made
/// PolterType the one foreign object in a row of flat panel icons
/// (issue #54). The two letters still say which layout it is.
#[test]
fn mono_draws_letters_on_nothing() {
    let mono = |id: &str, p: PanelPolarity| {
        let l = LayoutId::from(id);
        render_bare(layout_short_code(&l).as_bytes(), p)
    };
    let en = mono("en-US", PanelPolarity::Dark);
    assert_eq!(
        unit(&en, 0, 0),
        TRANSPARENT,
        "the corners are the panel, not a badge"
    );
    assert_ne!(
        en,
        mono("uk-UA", PanelPolarity::Dark),
        "the code still names the layout"
    );
    assert_ne!(
        en,
        mono("en-US", PanelPolarity::Light),
        "and it is drawn the other way round on a light panel"
    );
}

/// The halo is the insurance on a guess: the polarity comes from the
/// desktop's preference, and a panel is free to disagree with it. Every
/// letter therefore carries an edge in the other polarity, so a wrong
/// guess costs contrast rather than the icon.
#[test]
fn mono_letters_are_haloed_in_the_other_polarity() {
    let buf = render_bare(b"EN", PanelPolarity::Dark);
    let mut letters = 0;
    let mut halo = 0;
    for y in 0..UNITS {
        for x in 0..UNITS {
            match unit(&buf, x, y) {
                p if p == MONO_ON_DARK => letters += 1,
                p if p == MONO_HALO_ON_DARK => halo += 1,
                _ => {}
            }
        }
    }
    assert!(letters > 0, "the letters are drawn");
    assert!(halo > 0, "and so is an edge around them");
}

#[test]
fn paused_icon_differs_from_active_icon() {
    // Compares the underlying render() output rather than the icons, to
    // avoid depending on `Icon` equality.
    let normal = render(b"EN", color_for(&LayoutId::from("en-US")));
    let mut paused = render(b"EN", PAUSED_BG);
    draw_pause_indicator(&mut paused, glyph_colour(PAUSED_BG));
    assert_ne!(normal, paused);
}

/// With no tile to grey, the bars are the whole of what says "paused" —
/// so they have to be there, and in the letters' own colour.
#[test]
fn a_paused_mono_icon_still_says_so() {
    let plain = render_bare(b"EN", PanelPolarity::Dark);
    let mut paused = plain.clone();
    draw_pause_indicator(&mut paused, MONO_ON_DARK);
    assert_ne!(plain, paused);
}

#[test]
fn the_waiting_badge_leaves_the_layout_code_alone() {
    // The icon's job is to name the layout; the badge is a guest on it.
    // Every unit the two glyphs occupy must read the same with the mark
    // as without, or a "UK" with drafts waiting is a different word.
    let plain = render(b"UK", [0x4F, 0x9D, 0xFF, 0xFF]);
    let mut marked = plain.clone();
    draw_waiting_badge(&mut marked);
    for y in 5..11 {
        for x in 3..12 {
            assert_eq!(
                unit(&plain, x, y),
                unit(&marked, x, y),
                "the badge touched the glyphs at {x},{y}"
            );
        }
    }
    // …and it does mark it somewhere, or it would be a no-op that passes.
    assert_ne!(plain, marked);
}
