//! The primitives every drawing in the table is built from.
//!
//! Each flag is a function of one point: `u` runs 0→1 across the box,
//! `v` 0→1 down it. Nothing here rasterises a shape into the buffer,
//! so a flag is written as what it *is* — bands, a cross, a disc —
//! rather than as a sequence of fills whose order has to be right.

use super::consts::*;
use crate::icon_render::{PanelPolarity, W, luminance};

/// An opaque colour from the six-digit hex the flag is specified in,
/// so the table reads as the colours themselves.
pub(crate) const fn rgb(hex: u32) -> [u8; 4] {
    [(hex >> 16) as u8, (hex >> 8) as u8, hex as u8, 0xFF]
}

/// Paint the flag box from a function of normalised coordinates,
/// sampled at pixel centres.
pub(crate) fn paint(buf: &mut [u8], f: impl Fn(f32, f32) -> [u8; 4]) {
    for y in 0..IH {
        let v = (y as f32 + 0.5) / IH as f32;
        for x in 0..IW {
            let u = (x as f32 + 0.5) / IW as f32;
            let i = (((IY + y) * W + IX + x) * 4) as usize;
            buf[i..i + 4].copy_from_slice(&f(u, v));
        }
    }
}

/// Equal horizontal bands, top to bottom.
pub(crate) fn bands_h(colours: &[[u8; 4]]) -> impl Fn(f32, f32) -> [u8; 4] + '_ {
    move |_, v| band(colours, v)
}

/// Equal vertical bands, hoist to fly.
pub(crate) fn bands_v(colours: &[[u8; 4]]) -> impl Fn(f32, f32) -> [u8; 4] + '_ {
    move |u, _| band(colours, u)
}

fn band(colours: &[[u8; 4]], t: f32) -> [u8; 4] {
    let n = colours.len();
    let i = ((t * n as f32) as usize).min(n - 1);
    colours[i]
}

/// Horizontal bands with relative heights — Spain's 1:2:1, Latvia's
/// 2:1:2.
pub(crate) fn weighted_h(bands: &[(f32, [u8; 4])]) -> impl Fn(f32, f32) -> [u8; 4] + '_ {
    let total: f32 = bands.iter().map(|(w, _)| w).sum();
    move |_, v| {
        let mut edge = 0.0;
        for (w, c) in bands {
            edge += w / total;
            if v < edge {
                return *c;
            }
        }
        bands.last().map_or(WHITE, |(_, c)| *c)
    }
}

/// The Nordic cross: an upright left of centre, a bar across the
/// middle, and — Norway and Iceland — a second cross inside the first.
pub(crate) fn nordic(
    field: [u8; 4],
    cross: [u8; 4],
    inner: Option<[u8; 4]>,
) -> impl Fn(f32, f32) -> [u8; 4] {
    move |u, v| {
        let on = |hw: f32, hh: f32| (u - CROSS_U).abs() < hw || (v - 0.5).abs() < hh;
        match inner {
            Some(c) if on(CROSS_HW * INNER_CROSS, CROSS_HH * INNER_CROSS) => c,
            _ if on(CROSS_HW, CROSS_HH) => cross,
            _ => field,
        }
    }
}

/// Is `(u, v)` inside a circle of radius `r`?
///
/// `r` is a fraction of the box **height**, and the horizontal
/// distance is scaled by [`ASPECT`], so what comes out is a circle
/// rather than an ellipse.
pub(crate) fn in_disc(u: f32, v: f32, cu: f32, cv: f32, r: f32) -> bool {
    let dx = (u - cu) * ASPECT;
    let dy = v - cv;
    dx * dx + dy * dy <= r * r
}

/// Even-odd test against a closed polygon given in box fractions.
pub(crate) fn in_poly(u: f32, v: f32, verts: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let mut j = verts.len().saturating_sub(1);
    for (i, &(xi, yi)) in verts.iter().enumerate() {
        let (xj, yj) = verts[j];
        if (yi > v) != (yj > v) && u < (xj - xi) * (v - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// The ten vertices of a five-pointed star, one point up.
pub(crate) fn star5(cu: f32, cv: f32, r: f32) -> [(f32, f32); 10] {
    let mut pts = [(0.0, 0.0); 10];
    for (k, p) in pts.iter_mut().enumerate() {
        let a = -std::f32::consts::FRAC_PI_2 + k as f32 * std::f32::consts::PI / 5.0;
        let rad = if k % 2 == 0 { r } else { r * STAR_WAIST };
        *p = (cu + rad * a.cos() / ASPECT, cv + rad * a.sin());
    }
    pts
}

/// The three vertices of an equilateral triangle, turned `rot`
/// radians from point-up.
pub(crate) fn tri(cu: f32, cv: f32, r: f32, rot: f32) -> [(f32, f32); 3] {
    let mut pts = [(0.0, 0.0); 3];
    for (k, p) in pts.iter_mut().enumerate() {
        let a = -std::f32::consts::FRAC_PI_2 + rot + k as f32 * std::f32::consts::TAU / 3.0;
        *p = (cu + r * a.cos() / ASPECT, cv + r * a.sin());
    }
    pts
}

/// Flatten the flag into the grey band a paused badge is tinted in.
///
/// A flag has no tile to grey and half of them are as loud paused as
/// running, so this — not a colour change — is what has to carry
/// "paused" before the bars are read.
pub(crate) fn desaturate(buf: &mut [u8]) {
    for_box(buf, |px| {
        let g = GREY_FLOOR + (luminance(px) * f32::from(GREY_RANGE)) as u8;
        [g, g, g, px[3]]
    });
}

/// Ring the drawing, so the flag keeps its shape on a panel of its
/// own edge colour.
pub(crate) fn edge(buf: &mut [u8], polarity: PanelPolarity) {
    let c = match polarity {
        PanelPolarity::Dark => EDGE_ON_DARK,
        PanelPolarity::Light => EDGE_ON_LIGHT,
    };
    let drawn = |x: u32, y: u32| (EDGE..FW - EDGE).contains(&x) && (EDGE..FH - EDGE).contains(&y);
    for y in 0..FH {
        for x in 0..FW {
            if !drawn(x, y) {
                let i = (((FY + y) * W + FX + x) * 4) as usize;
                buf[i..i + 4].copy_from_slice(&c);
            }
        }
    }
}

fn for_box(buf: &mut [u8], f: impl Fn([u8; 4]) -> [u8; 4]) {
    for y in 0..IH {
        for x in 0..IW {
            let i = (((IY + y) * W + IX + x) * 4) as usize;
            let px = [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]];
            buf[i..i + 4].copy_from_slice(&f(px));
        }
    }
}
