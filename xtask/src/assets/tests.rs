use super::*;

const N: u32 = 256;

/// Colour of the pixel covering design-space point (`u`, `v`).
fn at(buf: &[u8], size: u32, u: f32, v: f32) -> [u8; 4] {
    let n = size as usize;
    let x = ((u / UNITS) * size as f32) as usize;
    let y = ((v / UNITS) * size as f32) as usize;
    let i = (y.min(n - 1) * n + x.min(n - 1)) * 4;
    [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
}

#[test]
fn rejects_sizes_too_small_to_carry_the_mark() {
    let out = std::path::Path::new("unused.png");
    assert!(render_app_icon(31, out).is_err());
}

#[test]
fn buffer_is_rgba_and_square() {
    let buf = rasterise(N);
    assert_eq!(buf.len(), (N * N * 4) as usize);
}

#[test]
fn corners_are_transparent() {
    let buf = rasterise(N);
    // The keycap is inset 2 units on each side and 4 at the bottom,
    // and its corners are rounded — so all four canvas corners are
    // outside the artwork.
    for (u, v) in [(0.5, 0.5), (63.5, 0.5), (0.5, 63.5), (63.5, 63.5)] {
        assert_eq!(at(&buf, N, u, v)[3], 0, "corner ({u}, {v}) must be clear");
    }
}

#[test]
fn keycap_shows_face_bevel_above_and_side_wall_below() {
    let buf = rasterise(N);
    // y=4 is on the top face but above the well; y=58 is below the
    // face entirely, where only the side wall is left.
    assert_eq!(at(&buf, N, 32.0, 4.0), KEY_FACE);
    assert_eq!(at(&buf, N, 32.0, 58.0), KEY_SIDE);
    // Between them, the recessed well the ghost sits in.
    assert_eq!(at(&buf, N, 10.0, 20.0), KEY_SIDE);
}

#[test]
fn ghost_body_eyes_and_smile_are_drawn() {
    let buf = rasterise(N);
    assert_eq!(at(&buf, N, 32.0, 22.0), GHOST, "dome");
    assert_eq!(at(&buf, N, 32.0, 33.0), GHOST, "body between the eyes");
    assert_eq!(at(&buf, N, EYE_LEFT_X, EYE_Y), INK, "left eye");
    assert_eq!(at(&buf, N, EYE_RIGHT_X, EYE_Y), INK, "right eye");
    assert_eq!(at(&buf, N, 32.0, 37.5), INK, "bottom of the smile");
    assert_eq!(at(&buf, N, 29.5, 36.5), INK, "left end of the smile");
    assert_eq!(at(&buf, N, 32.0, 35.0), GHOST, "inside the smile's arc");
}

#[test]
fn hem_alternates_lobes_and_notches() {
    let buf = rasterise(N);
    // A line low in the skirt crosses ghost on every lobe and bare
    // well in every notch. Without that alternation the ghost would
    // have a straight hem.
    let y = 45.0;
    for (u, want, what) in [
        (19.5, GHOST, "first lobe"),
        (23.2, KEY_SIDE, "left notch"),
        (28.1, GHOST, "second lobe"),
        (32.0, KEY_SIDE, "middle notch"),
        (35.9, GHOST, "third lobe"),
        (40.8, KEY_SIDE, "right notch"),
        (44.5, GHOST, "fourth lobe"),
    ] {
        assert_eq!(at(&buf, N, u, y), want, "{what} at x={u}");
    }
}

#[test]
fn hem_notches_are_deep_but_do_not_cut_into_the_body() {
    let buf = rasterise(N);
    // The middle notch reaches up to y=44; a hair above that the body
    // is still solid all the way across.
    assert_eq!(at(&buf, N, 32.0, 43.5), GHOST);
    assert_eq!(at(&buf, N, 32.0, 45.5), KEY_SIDE);
}

#[test]
fn outer_edge_is_anti_aliased() {
    let buf = rasterise(N);
    let partial = buf
        .chunks_exact(4)
        .filter(|px| px[3] > 0 && px[3] < 255)
        .count();
    assert!(
        partial > 100,
        "expected a soft outline, found {partial} partly-covered pixels"
    );
}

#[test]
fn small_sizes_keep_the_ghost() {
    // 32 px is the floor `render_app_icon` accepts, and the size the
    // adaptive sampler is deliberately switched off at — the smile is
    // half a pixel wide there.
    let buf = rasterise(32);
    assert_eq!(at(&buf, 32, 32.0, 22.0), GHOST);
    assert_eq!(at(&buf, 32, 32.0, 4.0), KEY_FACE);
}
