//! Windows layout-backend tests.
//!
//! These talk to the real OS — they read the keyboards this machine
//! actually has rather than a fixture. That is deliberate: the bug
//! behind issue #20 was invisible to any test that mocked the
//! keyboard, because the wrong answer was a perfectly well-formed
//! mapping. It just belonged to somebody else's keyboard.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;

/// How many `(plain, shift)` positions the two keyboards disagree on,
/// counting a key present in one and absent from the other as two.
fn positions_differing(a: &OsKeymap, b: &OsKeymap) -> usize {
    let index = |km: &OsKeymap| -> HashMap<u32, (char, Option<char>)> {
        km.keys.iter().map(|&(sc, p, s)| (sc, (p, s))).collect()
    };
    let (left, right) = (index(a), index(b));
    let mut scancodes: Vec<u32> = left.keys().chain(right.keys()).copied().collect();
    scancodes.sort_unstable();
    scancodes.dedup();

    scancodes
        .into_iter()
        .map(|sc| {
            let (lp, ls) = left.get(&sc).map_or((None, None), |&(p, s)| (Some(p), s));
            let (rp, rs) = right.get(&sc).map_or((None, None), |&(p, s)| (Some(p), s));
            usize::from(lp != rp) + usize::from(ls != rs)
        })
        .sum()
}

/// Whatever this machine has installed, each keyboard has to describe
/// as a *whole* keyboard — that is the promise the layout DB relies on
/// when it replaces a bundled table instead of merging into it.
#[test]
fn installed_keyboards_describe_a_full_character_block() {
    let maps = WindowsLayoutSwitcher::new()
        .describe_keymaps()
        .expect("describe the installed keyboards");

    if maps.is_empty() {
        // A session with no keyboard layouts at all — possible on a
        // bare CI image. Nothing to assert, and nothing wrong.
        return;
    }

    for km in &maps {
        assert!(
            km.keys.len() >= 40,
            "{} ({}) described only {} keys — a real keyboard fills nearly all \
             {} of the character block",
            km.id,
            km.variant,
            km.keys.len(),
            CHARACTER_SCANCODES.len()
        );

        let mut seen = std::collections::HashSet::new();
        for &(scancode, plain, shift) in &km.keys {
            assert!(
                CHARACTER_SCANCODES.contains(&scancode),
                "{}: 0x{scancode:X} was never asked about",
                km.id
            );
            assert!(
                seen.insert(scancode),
                "{}: 0x{scancode:X} described twice — the DB keys a map on this",
                km.id
            );
            assert!(
                !plain.is_control(),
                "{}: 0x{scancode:X} produced a control character",
                km.id
            );
            assert_ne!(
                shift,
                Some(plain),
                "{}: 0x{scancode:X} should carry `None` when Shift changes nothing",
                km.id
            );
        }
    }
}

/// The measurement behind #20, taken through the shipping code path.
///
/// Windows ships three genuinely different Bulgarian keyboards under
/// one LCID, so all three arrive as `bg-BG` and only one can match
/// `bg_bg.toml`. This test says we can tell them apart at all, and
/// prints each table so the numbers in #20 can be re-derived.
///
/// Ignored by default: it loads keymap DLLs into the process and needs
/// those three keyboards installed. `LoadKeyboardLayoutW` also adds
/// them to the **session's** layout list until you log out — so a
/// PolterType started afterwards sees three Bulgarian keyboards the
/// user never chose. That is this test's footprint, not a bug; the
/// persistent list under `HKCU\Keyboard Layout\Preload` is untouched.
///
/// ```text
/// cargo test -p poltertype-layout -- --ignored --nocapture bulgarian
/// ```
#[test]
#[ignore = "loads keyboard layouts into the process; run by hand"]
fn the_three_bulgarian_keyboards_describe_differently() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{KLF_NOTELLSHELL, LoadKeyboardLayoutW};
    use windows::core::PCWSTR;

    const VARIANTS: &[(&str, &str)] = &[
        ("00030402", "Phonetic Traditional"),
        ("00000402", "Typewriter"),
        ("00040402", "Phonetic"),
    ];

    let mut described = Vec::new();
    for (klid, name) in VARIANTS {
        let wide: Vec<u16> = klid.encode_utf16().chain(std::iter::once(0)).collect();
        // Safety: `wide` is NUL-terminated and outlives the call.
        // `KLF_NOTELLSHELL` keeps this process-local — the user's own
        // keyboard list is not touched.
        let hkl = unsafe { LoadKeyboardLayoutW(PCWSTR(wide.as_ptr()), KLF_NOTELLSHELL) }
            .unwrap_or_else(|e| panic!("load Bulgarian {name} ({klid}): {e}"));

        let km = describe_hkl(hkl);
        println!(
            "\n--- {klid}  Bulgarian ({name})  {} keys ---",
            km.keys.len()
        );
        let mut rows = km.keys.clone();
        rows.sort_unstable_by_key(|&(sc, _, _)| sc);
        for (sc, plain, shift) in rows {
            match shift {
                Some(s) => println!("0x{sc:02X} = {{ plain = \"{plain}\", shift = \"{s}\" }}"),
                None => println!("0x{sc:02X} = {{ plain = \"{plain}\" }}"),
            }
        }
        described.push((*name, km));
    }

    // The heart of #20: one id, three keyboards.
    for (name, km) in &described {
        assert_eq!(
            km.id,
            LayoutId::from("bg-BG"),
            "Bulgarian {name} should still report as bg-BG — that is the whole problem"
        );
    }

    let traditional = &described[0].1;
    let typewriter = &described[1].1;
    let phonetic = &described[2].1;

    let vs_typewriter = positions_differing(traditional, typewriter);
    let vs_phonetic = positions_differing(traditional, phonetic);
    println!("\nPhonetic Traditional vs Typewriter: {vs_typewriter} positions differ");
    println!("Phonetic Traditional vs Phonetic:   {vs_phonetic} positions differ");

    assert!(
        vs_typewriter > 0,
        "Typewriter is a different keyboard and must not describe identically"
    );
    assert!(
        vs_phonetic > 50,
        "Phonetic differs from Phonetic Traditional across most of the alphabet; \
         got only {vs_phonetic} differing positions, which means the description \
         is not variant-aware after all"
    );
}
