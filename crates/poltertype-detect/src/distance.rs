//! Weighted optimal-string-alignment distance between a typed token
//! and a candidate word, keyboard-geometry aware.

use crate::consts::TRANSPOSITION_COST;
use crate::geometry::KeyboardGeometry;

/// Substitution cost for `a` → `b`: graded by physical key distance,
/// so `hwllo` prefers `hello` (w↔e are direct neighbours) over
/// `hallo` (w↔a are diagonal neighbours). Beyond 1.5 key units it's
/// a full-price substitution — that's not a finger slip any more.
pub(crate) fn substitution_cost(geo: Option<&KeyboardGeometry>, a: char, b: char) -> f32 {
    if a == b {
        return 0.0;
    }
    match geo.and_then(|g| g.proximity_sq(a, b)) {
        Some(d2) if d2 <= 2.25 => 0.3 + 0.1 * d2,
        _ => 1.0,
    }
}

/// Weighted optimal-string-alignment (restricted Damerau-Levenshtein)
/// distance. Costs: exact match 0, substitution per
/// [`substitution_cost`], adjacent transposition [`TRANSPOSITION_COST`],
/// insertion/deletion 1.
pub(crate) fn weighted_osa(typed: &[char], cand: &[char], geo: Option<&KeyboardGeometry>) -> f32 {
    let n = typed.len();
    let m = cand.len();
    if n == 0 {
        return m as f32;
    }
    if m == 0 {
        return n as f32;
    }

    // Three rolling rows of the DP matrix (previous-previous is needed
    // for the transposition case).
    let mut prev2 = vec![0.0f32; m + 1];
    let mut prev = vec![0.0f32; m + 1];
    let mut cur = vec![0.0f32; m + 1];
    for (j, slot) in prev.iter_mut().enumerate() {
        *slot = j as f32;
    }

    for i in 1..=n {
        cur[0] = i as f32;
        for j in 1..=m {
            let a = typed[i - 1];
            let b = cand[j - 1];
            let sub_cost = substitution_cost(geo, a, b);
            let mut best = (prev[j] + 1.0) // deletion
                .min(cur[j - 1] + 1.0) // insertion
                .min(prev[j - 1] + sub_cost); // substitution / match
            if i > 1 && j > 1 && a == cand[j - 2] && typed[i - 2] == b && a != b {
                best = best.min(prev2[j - 2] + TRANSPOSITION_COST);
            }
            cur[j] = best;
        }
        std::mem::swap(&mut prev2, &mut prev);
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[m]
}
