use super::*;

/// Verbatim from the guest: `gsettings get
/// org.gnome.desktop.wm.keybindings switch-input-source` prints
/// `['<Super>space', 'XF86Keyboard']` on GNOME 49.
#[test]
fn the_gnome_default_is_super_space() {
    assert_eq!(
        parse_gtk_accelerator("<Super>space"),
        Some(SwitchChord {
            scancode: SC_SPACE,
            meta: true,
            ..Default::default()
        })
    );
}

#[test]
fn several_modifiers_stack() {
    assert_eq!(
        parse_gtk_accelerator("<Control><Shift>space"),
        Some(SwitchChord {
            scancode: SC_SPACE,
            ctrl: true,
            shift: true,
            ..Default::default()
        })
    );
}

/// A media key or a letter is not something to guess at: pressing
/// the wrong key on a user's desktop is worse than not pressing.
#[test]
fn a_key_we_cannot_name_yields_nothing() {
    assert!(parse_gtk_accelerator("XF86Keyboard").is_none());
    assert!(parse_gtk_accelerator("<Super>k").is_none());
    assert!(parse_gtk_accelerator("<Hyper>space").is_none());
}

/// Verbatim from the guest's `_XKB_RULES_NAMES`:
/// `grp:alt_shift_toggle,grp_led:scroll`.
#[test]
fn the_xkb_toggle_is_read_out_of_the_options_list() {
    assert_eq!(
        parse_xkb_group_option("grp:alt_shift_toggle,grp_led:scroll"),
        Some(SwitchChord {
            scancode: SC_LEFTSHIFT,
            alt: true,
            ..Default::default()
        })
    );
    assert_eq!(
        parse_xkb_group_option("terminate:ctrl_alt_bksp,grp:caps_toggle"),
        Some(SwitchChord {
            scancode: SC_CAPSLOCK,
            ..Default::default()
        })
    );
}

#[test]
fn options_without_a_group_toggle_yield_nothing() {
    assert!(parse_xkb_group_option("terminate:ctrl_alt_bksp,grp_led:scroll").is_none());
    assert!(parse_xkb_group_option("").is_none());
    // Both-shifts is real and deliberately unhandled.
    assert!(parse_xkb_group_option("grp:shifts_toggle").is_none());
}
