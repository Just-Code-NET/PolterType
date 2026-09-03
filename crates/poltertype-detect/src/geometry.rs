//! Physical keyboard geometry, for the suggestion ranker's
//! neighbour-aware substitution cost. See [`crate::suggest`].

use std::collections::HashMap;

/// Physical position of a key on the standard staggered board, in key
/// units: `(row, column)`. Derived purely from the Win SC Set-1
/// scancode — physical geometry is layout-independent, which is the
/// whole point of using scancodes as the canonical key identity.
fn scancode_grid_pos(sc: u32) -> Option<(f32, f32)> {
    match sc {
        // Digits row `1`..`=`.
        0x02..=0x0D => Some((0.0, (sc - 0x02) as f32)),
        // Top letter row `q`..`]`, staggered half a key right.
        0x10..=0x1B => Some((1.0, (sc - 0x10) as f32 + 0.5)),
        // Home row `a`..`'`.
        0x1E..=0x28 => Some((2.0, (sc - 0x1E) as f32 + 0.75)),
        // ANSI backslash / ISO extra home-row key next to Enter.
        0x2B => Some((2.0, 11.75)),
        // Bottom row `z`..`/`.
        0x2C..=0x35 => Some((3.0, (sc - 0x2C) as f32 + 1.25)),
        // ISO 102nd key (`<>|`, left of Z on European boards).
        0x56 => Some((3.0, 0.25)),
        _ => None,
    }
}

/// Per-layout map from produced character to physical key position —
/// what lets the ranking metric see that `слоао` is one finger-slip
/// (`а` sits next to `в`) away from `слово`.
#[derive(Debug, Default, Clone)]
pub struct KeyboardGeometry {
    pos: HashMap<char, (f32, f32)>,
}

impl KeyboardGeometry {
    /// Build from `(scancode, produced char)` pairs — callers feed
    /// both the plain and the shifted character of every mapped key
    /// (lowercased; ranking runs on canonicalised tokens).
    pub fn from_scancode_chars<I>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (u32, char)>,
    {
        let mut pos = HashMap::new();
        for (sc, ch) in pairs {
            if let Some(p) = scancode_grid_pos(sc) {
                for low in ch.to_lowercase() {
                    pos.entry(low).or_insert(p);
                }
            }
        }
        Self { pos }
    }

    /// Squared physical distance between the keys producing `a` and
    /// `b`, in key units. `None` when either char isn't on the grid.
    pub(crate) fn proximity_sq(&self, a: char, b: char) -> Option<f32> {
        let (&(ra, ca), &(rb, cb)) = (self.pos.get(&a)?, self.pos.get(&b)?);
        let dr = ra - rb;
        let dc = ca - cb;
        Some(dr * dr + dc * dc)
    }
}
