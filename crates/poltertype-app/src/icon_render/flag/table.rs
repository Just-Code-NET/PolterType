//! One drawing per country, in the flag's own terms.
//!
//! **A region is listed only if its flag can be told apart from the
//! others here at a panel's size.** Slovakia, Slovenia, Serbia and
//! Croatia are white-blue-red bands plus an emblem no 48-pixel drawing
//! can carry, and Mexico is Italy plus one; drawn as bands they would
//! each be a confident lie about the layout in force. They are absent
//! on purpose, and an absent region keeps the lettered badge — see
//! `region_of`.
//!
//! Emblems that *are* drawn are simplified to what survives the size:
//! the United States has twenty stars rather than fifty, Portugal's
//! armillary sphere is three rings, Korea's taegeuk is split on a
//! straight line. Every one of them is still the only flag in the
//! table that looks like that.

use super::consts::*;
use super::paint::*;

/// Draw `region`'s flag into `buf`, or answer `false` for a country
/// with no drawing here.
pub(crate) fn draw(buf: &mut [u8], region: &str) -> bool {
    match region {
        // ─── Horizontal bands ────────────────────────────────────────
        "UA" => paint(buf, bands_h(&[rgb(0x0057B7), rgb(0xFFD700)])),
        "PL" => paint(buf, bands_h(&[WHITE, rgb(0xDC143C)])),
        "ID" => paint(buf, bands_h(&[rgb(0xCE1126), WHITE])),
        "RU" => paint(buf, bands_h(&[WHITE, rgb(0x0039A6), rgb(0xD52B1E)])),
        "DE" => paint(buf, bands_h(&[BLACK, rgb(0xDD0000), rgb(0xFFCE00)])),
        "NL" => paint(buf, bands_h(&[rgb(0xAE1C28), WHITE, rgb(0x21468B)])),
        "HU" => paint(buf, bands_h(&[rgb(0xCE2939), WHITE, rgb(0x477050)])),
        "BG" => paint(buf, bands_h(&[WHITE, rgb(0x00966E), rgb(0xD62612)])),
        "AT" => paint(buf, bands_h(&[rgb(0xED2939), WHITE, rgb(0xED2939)])),
        "EE" => paint(buf, bands_h(&[rgb(0x0072CE), BLACK, WHITE])),
        "LT" => paint(buf, bands_h(&[rgb(0xFDB913), rgb(0x006A44), rgb(0xC1272D)])),
        "AM" => paint(buf, bands_h(&[rgb(0xD90012), rgb(0x0033A0), rgb(0xF2A800)])),

        // ─── Vertical bands ──────────────────────────────────────────
        "FR" => paint(buf, bands_v(&[rgb(0x002395), WHITE, rgb(0xED2939)])),
        "IT" => paint(buf, bands_v(&[rgb(0x008C45), rgb(0xF4F5F0), rgb(0xCD212A)])),
        "RO" => paint(buf, bands_v(&[rgb(0x002B7F), rgb(0xFCD116), rgb(0xCE1126)])),
        "IE" => paint(buf, bands_v(&[rgb(0x169B62), WHITE, rgb(0xFF883E)])),
        "BE" => paint(buf, bands_v(&[BLACK, rgb(0xFAE042), rgb(0xED2939)])),

        // ─── Bands of unequal height ─────────────────────────────────
        "ES" => paint(
            buf,
            weighted_h(&[
                (1.0, rgb(0xAA151B)),
                (2.0, rgb(0xF1BF00)),
                (1.0, rgb(0xAA151B)),
            ]),
        ),
        "LV" => paint(
            buf,
            weighted_h(&[(2.0, rgb(0x9E3039)), (1.0, WHITE), (2.0, rgb(0x9E3039))]),
        ),

        // ─── Nordic crosses ──────────────────────────────────────────
        "DK" => paint(buf, nordic(rgb(0xC60C30), WHITE, None)),
        "SE" => paint(buf, nordic(rgb(0x006AA7), rgb(0xFECC00), None)),
        "FI" => paint(buf, nordic(WHITE, rgb(0x003580), None)),
        "NO" => paint(buf, nordic(rgb(0xBA0C2F), WHITE, Some(rgb(0x00205B)))),
        "IS" => paint(buf, nordic(rgb(0x02529C), WHITE, Some(rgb(0xDC1E35)))),

        // ─── A cross in the middle ───────────────────────────────────
        "CH" => paint(buf, |u, v| {
            let up = (u - 0.5).abs() < 0.075 && (v - 0.5).abs() < 0.30;
            let across = (v - 0.5).abs() < 0.10 && (u - 0.5).abs() < 0.225;
            if up || across { WHITE } else { rgb(0xD52B1E) }
        }),
        "GE" => paint(buf, |u, v| {
            let big = (u - 0.5).abs() < 0.085 || (v - 0.5).abs() < 0.113;
            // One Bolnisi cross per quarter, each clear of the big one.
            let (qu, qv) = (
                if u < 0.5 { 0.25 } else { 0.75 },
                if v < 0.5 { 0.25 } else { 0.75 },
            );
            let small = ((u - qu).abs() < 0.030 && (v - qv).abs() < 0.110)
                || ((v - qv).abs() < 0.040 && (u - qu).abs() < 0.083);
            if big || small { rgb(0xFF0000) } else { WHITE }
        }),

        // ─── Stripes under a canton ──────────────────────────────────
        "US" => paint(buf, |u, v| {
            const CANTON_U: f32 = 0.40;
            const CANTON_V: f32 = 7.0 / 13.0;
            if u < CANTON_U && v < CANTON_V {
                // Twenty stars, as dots: fifty is grey at this size.
                let du = (u / CANTON_U * 5.0).fract() - 0.5;
                let dv = (v / CANTON_V * 4.0).fract() - 0.5;
                if du * du + dv * dv < 0.045 {
                    WHITE
                } else {
                    rgb(0x3C3B6E)
                }
            } else if (v * 13.0) as u32 % 2 == 0 {
                rgb(0xB22234)
            } else {
                WHITE
            }
        }),
        "GR" => paint(buf, |u, v| {
            // The canton is a square five stripes tall, so its width
            // in `u` and its height in `v` differ by the box's aspect.
            const CU: f32 = 5.0 / 9.0 / ASPECT;
            const CV: f32 = 5.0 / 9.0;
            let blue = rgb(0x0D5EAF);
            if u < CU && v < CV {
                let cross = (u - CU / 2.0).abs() < 0.055 || (v - CV / 2.0).abs() < 0.073;
                if cross { WHITE } else { blue }
            } else if (v * 9.0) as u32 % 2 == 0 {
                blue
            } else {
                WHITE
            }
        }),

        // ─── A disc on a plain field ─────────────────────────────────
        "JP" => paint(buf, |u, v| {
            if in_disc(u, v, 0.5, 0.5, 0.30) {
                rgb(0xBC002D)
            } else {
                WHITE
            }
        }),
        "AR" => paint(buf, |u, v| {
            if in_disc(u, v, 0.5, 0.5, 0.11) {
                rgb(0xF6B40E)
            } else if v < 1.0 / 3.0 || v > 2.0 / 3.0 {
                rgb(0x74ACDF)
            } else {
                WHITE
            }
        }),
        "KZ" => paint(buf, |u, v| {
            let gold = rgb(0xFEC50C);
            // The hoist ornament, reduced to the gold band it reads as.
            if u < 0.06 || in_disc(u, v, 0.53, 0.44, 0.17) {
                gold
            } else {
                rgb(0x00AFCA)
            }
        }),

        // ─── The rest, each its own shape ────────────────────────────
        "GB" => paint(buf, |u, v| {
            // The saltires are measured in the box's own coordinates,
            // which stretches them exactly as the field does.
            let d = (u - v).abs().min((u + v - 1.0).abs());
            let (up, across) = ((u - 0.5).abs(), (v - 0.5).abs());
            if up < 0.062 || across < 0.083 {
                rgb(0xC8102E)
            } else if up < 0.104 || across < 0.139 {
                WHITE
            } else if d < 0.055 {
                rgb(0xC8102E)
            } else if d < 0.130 {
                WHITE
            } else {
                rgb(0x012169)
            }
        }),
        "CZ" => paint(buf, |u, v| {
            if u < 0.5 && (v - 0.5).abs() <= 0.5 - u {
                rgb(0x11457E)
            } else if v < 0.5 {
                WHITE
            } else {
                rgb(0xD7141A)
            }
        }),
        "PT" => paint(buf, |u, v| {
            if in_disc(u, v, 0.40, 0.5, 0.06) {
                WHITE
            } else if in_disc(u, v, 0.40, 0.5, 0.13) {
                rgb(0xFF0000)
            } else if in_disc(u, v, 0.40, 0.5, 0.20) {
                rgb(0xFFDD00)
            } else if u < 0.40 {
                rgb(0x006600)
            } else {
                rgb(0xFF0000)
            }
        }),
        "BR" => paint(buf, |u, v| {
            let rhombus = (u - 0.5).abs() / 0.44 + (v - 0.5).abs() / 0.42;
            if in_disc(u, v, 0.5, 0.5, 0.17) {
                rgb(0x002776)
            } else if rhombus <= 1.0 {
                rgb(0xFEDF00)
            } else {
                rgb(0x009B3A)
            }
        }),
        "BY" => paint(buf, |u, v| {
            if u < 0.14 {
                // The hoist ornament, reduced to its red-on-white beat.
                let du = (u / 0.14 * 2.0).fract() - 0.5;
                let dv = (v * 7.0).fract() - 0.5;
                if du.abs() + dv.abs() < 0.36 {
                    rgb(0xCE1720)
                } else {
                    WHITE
                }
            } else if v < 2.0 / 3.0 {
                rgb(0xCE1720)
            } else {
                rgb(0x007C30)
            }
        }),
        "TR" => {
            let star = star5(0.62, 0.5, 0.15);
            paint(buf, move |u, v| {
                let crescent = in_disc(u, v, 0.36, 0.5, 0.26) && !in_disc(u, v, 0.42, 0.5, 0.21);
                if crescent || in_poly(u, v, &star) {
                    WHITE
                } else {
                    rgb(0xE30A17)
                }
            });
        }
        "IL" => {
            let (up, down) = (
                tri(0.5, 0.5, 0.26, 0.0),
                tri(0.5, 0.5, 0.26, std::f32::consts::PI),
            );
            paint(buf, move |u, v| {
                let stripes = (v - 0.20).abs() < 0.055 || (v - 0.80).abs() < 0.055;
                if stripes || in_poly(u, v, &up) || in_poly(u, v, &down) {
                    rgb(0x0038B8)
                } else {
                    WHITE
                }
            });
        }
        "KR" => paint(buf, |u, v| {
            if in_disc(u, v, 0.5, 0.5, 0.24) {
                // The taegeuk's S-curve is below this resolution; the
                // diagonal it turns about is not.
                if (u - 0.5) * ASPECT + (v - 0.5) < 0.0 {
                    rgb(0xCD2E3A)
                } else {
                    rgb(0x0047A0)
                }
            } else if trigram(u, v) {
                BLACK
            } else {
                WHITE
            }
        }),

        _ => return false,
    }
    true
}

/// The four groups of bars in Korea's corners, upright rather than
/// turned: three 2-pixel bars is all the corner has room for.
fn trigram(u: f32, v: f32) -> bool {
    let (cu, cv) = (
        if u < 0.5 { 0.17 } else { 0.83 },
        if v < 0.5 { 0.20 } else { 0.80 },
    );
    (u - cu).abs() < 0.055 && (v - cv).abs() < 0.105 && ((v - cv + 0.105) / 0.042) as u32 % 2 == 0
}
