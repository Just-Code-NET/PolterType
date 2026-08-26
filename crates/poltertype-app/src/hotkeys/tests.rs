//! The substitution rules. Every case here is one the tray and the
//! Settings window must answer identically — they answered differently
//! in v0.17.4 and it cost a user their force-switch key (issue #31).

use super::*;

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
        swapped.pause,
        parse_hotkey_or_default(WAYLAND_SAFE_SWITCH_LAST, DEFAULT_PAUSE_TOGGLE)
    );
    assert_eq!(
        swapped.switch_last,
        parse_hotkey_or_default(DEFAULT_PAUSE_TOGGLE, DEFAULT_SWITCH_LAST)
    );

    let mut sent = Vec::new();
    while let Ok(cmd) = rx.try_recv() {
        if let EngineCommand::SetKeystreamHotkeys(hk) = cmd {
            sent.push((
                hk.pause.map(|c| c.scancode),
                hk.switch_last.map(|c| c.scancode),
            ));
        }
    }
    assert_eq!(sent.len(), 2, "each apply must re-arm the key stream");
    // F9 and Space as SC Set-1 — the two the second call asked for, in
    // the order the swap put them.
    assert_eq!(sent[1], (Some(0x43), Some(0x39)));
}
