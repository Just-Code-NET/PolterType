//! The substitution rules. Every case here is one the tray and the
//! Settings window must answer identically — they answered differently
//! in v0.17.4 and it cost a user their force-switch key (issue #31).

use super::*;

fn key_scancode(b: Option<Binding>) -> Option<u32> {
    match b {
        Some(Binding::Key(c)) => Some(c.scancode),
        _ => None,
    }
}

const WAYLAND: HotkeyEnvironment = HotkeyEnvironment {
    observed_not_consumed: true,
    system_owns_ctrl_shift_space: false,
};
const MACOS: HotkeyEnvironment = HotkeyEnvironment {
    observed_not_consumed: false,
    system_owns_ctrl_shift_space: true,
};
const PLAIN: HotkeyEnvironment = HotkeyEnvironment {
    observed_not_consumed: false,
    system_owns_ctrl_shift_space: false,
};

#[test]
fn the_default_switch_last_is_replaced_only_where_it_would_delete_the_word() {
    let here = effective_switch_last(DEFAULT_SWITCH_LAST, WAYLAND);
    assert_eq!(here.chord, WAYLAND_SAFE_SWITCH_LAST);
    assert_eq!(
        here.substitution,
        Some(Substitution::DefaultIsDestructiveHere)
    );

    for env in [PLAIN, MACOS] {
        let elsewhere = effective_switch_last(DEFAULT_SWITCH_LAST, env);
        assert_eq!(elsewhere.chord, DEFAULT_SWITCH_LAST);
        assert_eq!(elsewhere.substitution, None);
    }
}

#[test]
fn the_default_pause_is_replaced_only_where_the_system_owns_it() {
    let here = effective_pause_toggle(DEFAULT_PAUSE_TOGGLE, MACOS);
    assert_eq!(here.chord, MACOS_SAFE_PAUSE_TOGGLE);
    assert_eq!(here.substitution, Some(Substitution::SystemOwnsDefault));

    for env in [PLAIN, WAYLAND] {
        let elsewhere = effective_pause_toggle(DEFAULT_PAUSE_TOGGLE, env);
        assert_eq!(elsewhere.chord, DEFAULT_PAUSE_TOGGLE);
        assert_eq!(elsewhere.substitution, None);
    }
}

#[test]
fn an_explicit_binding_is_never_second_guessed() {
    // The whole point of the "still on the default" condition: someone
    // who chose Ctrl+Shift+Backspace on Wayland meant it.
    for env in [WAYLAND, MACOS, PLAIN] {
        let switch = effective_switch_last("Ctrl+Alt+K", env);
        assert_eq!(switch.chord, "Ctrl+Alt+K");
        assert_eq!(switch.substitution, None);

        let pause = effective_pause_toggle("Ctrl+Alt+J", env);
        assert_eq!(pause.chord, "Ctrl+Alt+J");
        assert_eq!(pause.substitution, None);
    }
    let deliberate = effective_switch_last(DEFAULT_SWITCH_LAST, PLAIN);
    assert_eq!(deliberate.chord, DEFAULT_SWITCH_LAST);
}

#[test]
fn every_chord_we_substitute_survives_the_round_trip_to_a_scancode() {
    // A substitute that does not map to SC Set-1 is silently unbound on
    // the keystream backend — which is exactly the failure this whole
    // module exists to stop being silent about.
    for chord in [WAYLAND_SAFE_SWITCH_LAST, MACOS_SAFE_PAUSE_TOGGLE] {
        let mapped = chord
            .parse::<HotKey>()
            .ok()
            .and_then(|hk| chord_from_hotkey(&hk));
        assert!(
            mapped.is_some(),
            "{chord} must parse and map to an SC Set-1 scancode, or it is unbound on Wayland"
        );
    }
}

/// Re-applying is the whole point: the chords used to be resolved once
/// before the event loop, so a hotkey changed in the Settings window
/// did nothing until the app was restarted (issue #34).
#[test]
fn re_applying_puts_the_new_chords_on_the_key_stream() {
    let (tx, rx) = crossbeam_channel::unbounded();

    let first = apply_hotkeys(
        DEFAULT_PAUSE_TOGGLE,
        DEFAULT_SWITCH_LAST,
        WAYLAND,
        true,
        None,
        &tx,
        None,
    );
    let swapped = apply_hotkeys(
        WAYLAND_SAFE_SWITCH_LAST,
        DEFAULT_PAUSE_TOGGLE,
        WAYLAND,
        true,
        None,
        &tx,
        Some(first),
    );

    assert_eq!(
        swapped.pause.os_grab(),
        Some(parse_hotkey_or_default(
            WAYLAND_SAFE_SWITCH_LAST,
            DEFAULT_PAUSE_TOGGLE
        ))
    );
    assert_eq!(
        swapped.switch_last.os_grab(),
        Some(parse_hotkey_or_default(
            DEFAULT_PAUSE_TOGGLE,
            DEFAULT_SWITCH_LAST
        ))
    );

    let mut sent = Vec::new();
    while let Ok(cmd) = rx.try_recv() {
        if let EngineCommand::SetKeystreamHotkeys(hk) = cmd {
            sent.push((key_scancode(hk.pause), key_scancode(hk.switch_last)));
        }
    }
    assert_eq!(sent.len(), 2, "each apply must re-arm the key stream");
    // F9 and Space as SC Set-1 — the two the second call asked for, in
    // the order the swap put them.
    assert_eq!(sent[1], (Some(0x43), Some(0x39)));
}

/// `HotKey::new` normalises META to SUPER, so a chord built from the
/// Super key carried `meta: false` and could never match on the
/// keystream backends — every Wayland/evdev machine, and the
/// suggestion digits everywhere.
#[test]
fn a_super_chord_reaches_the_key_stream_with_its_modifier_intact() {
    let hk = parse_hotkey_or_default("Ctrl+Super+K", DEFAULT_PAUSE_TOGGLE);
    let chord = chord_from_hotkey(&hk);

    assert_eq!(
        chord.map(|c| (c.ctrl, c.meta, c.scancode)),
        Some((true, true, 0x25))
    );
}

/// The two shapes a modifier-only binding may take, and the ones that
/// must stay ordinary hotkey strings.
#[test]
fn modifier_only_chords_parse_into_the_two_shapes_and_nothing_else() {
    let ctrl_shift = parse_mod_chord("Ctrl+Shift");
    assert_eq!(
        ctrl_shift.map(|m| (m.mods.ctrl, m.mods.shift, m.double_tap)),
        Some((true, true, false))
    );
    let double = parse_mod_chord("Shift+Shift");
    assert_eq!(
        double.map(|m| (m.mods.shift, m.mods.count(), m.double_tap)),
        Some((true, 1, true))
    );
    assert_eq!(parse_mod_chord("cmd+alt"), parse_mod_chord("Super+Alt"));

    for refused in [
        // A lone modifier: Shift+click is invisible to us on Windows
        // and macOS, and would fire it.
        "Shift",
        "Ctrl",
        // Three taps is not a gesture anyone asked for.
        "Shift+Shift+Shift",
        "Ctrl+Ctrl+Shift",
        // Ordinary hotkeys, which take the other road.
        "Ctrl+Shift+Space",
        "F9",
        "",
    ] {
        assert_eq!(parse_mod_chord(refused), None, "{refused}");
    }
}

/// A modifier-only chord cannot be an OS-level grab, having no key code
/// to register, so it has to reach the key stream even on the backends
/// where ordinary chords are the OS's business.
#[test]
fn a_modifier_chord_goes_to_the_key_stream_where_a_key_chord_does_not() {
    let (tx, rx) = crossbeam_channel::unbounded();

    let active = apply_hotkeys(
        DEFAULT_PAUSE_TOGGLE,
        "Shift+Shift",
        PLAIN,
        false,
        None,
        &tx,
        None,
    );
    assert!(active.pause.os_grab().is_some(), "pause stays an OS grab");
    assert!(
        active.switch_last.os_grab().is_none(),
        "a modifier chord must never be handed to the OS grab"
    );

    let sent = rx.try_recv().ok();
    let Some(EngineCommand::SetKeystreamHotkeys(hk)) = sent else {
        unreachable!("no keystream hotkeys were sent")
    };
    assert_eq!(hk.pause, None, "the OS holds the pause chord here");
    assert!(
        matches!(hk.switch_last, Some(Binding::Mods(m)) if m.double_tap && m.mods.shift),
        "got {:?}",
        hk.switch_last
    );
}
