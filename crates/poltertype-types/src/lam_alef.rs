//! The one case of a key that types two code points.
//!
//! A key table entry is one `char`, and Arabic's lam-alef keys break that:
//! macOS and Windows answer `B` with `لا`, U+0644 U+0627. On Linux xkb's
//! `ara` emits the presentation form U+FEFB and libX11's Compose table,
//! which GTK honours, turns that back into the same pair. So the table
//! carries the form, and the screen holds two code points.

const LIGATURES: [(u16, char); 4] = [
    (0x0627, '\u{FEFB}'),
    (0x0623, '\u{FEF7}'),
    (0x0625, '\u{FEF9}'),
    (0x0622, '\u{FEF5}'),
];
const LAM: u16 = 0x0644;

/// The presentation form of a lam-alef pair, for a backend reading what a
/// key produces. `None` for anything else.
pub fn fold_lam_alef(utf16: &[u16]) -> Option<char> {
    match utf16 {
        [LAM, alef] => LIGATURES
            .iter()
            .find(|(a, _)| a == alef)
            .map(|&(_, form)| form),
        _ => None,
    }
}

/// The two code points a presentation form stands for.
pub fn lam_alef_pair(form: char) -> Option<[char; 2]> {
    let &(alef, _) = LIGATURES.iter().find(|&&(_, f)| f == form)?;
    Some(['\u{0644}', char::from_u32(u32::from(alef))?])
}
