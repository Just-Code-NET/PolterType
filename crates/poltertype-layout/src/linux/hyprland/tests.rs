use super::*;
use crate::LayoutId;

/// Trimmed real `hyprctl devices` output from a Hyprland + keyd
/// machine in the desynced state that broke en→uk corrections: the
/// user's Alt+Shift toggle moved only the keyd virtual keyboard to
/// Ukrainian, every other device (including our emitter, promoted to
/// `main`) still reports the layout of the last `switchxkblayout all`.
const DESYNCED: &str = r#"Keyboards:
	Keyboard at 55617d1313e0:
		logitech-mx-keys
			rules: r "", m "", l "us, ua", v ",", o "grp:alt_shift_toggle"
			active layout index: 0
			active keymap: English (US)
			main: no
	Keyboard at 55617d2d0480:
		keyd-virtual-keyboard
			rules: r "", m "", l "us, ua", v ",", o "grp:alt_shift_toggle"
			active layout index: 1
			active keymap: Ukrainian
			main: no
	Keyboard at 55617d2e1a20:
		poltertype-virtual-keyboard
			rules: r "", m "", l "us, ua", v ",", o "grp:alt_shift_toggle"
			active layout index: 0
			active keymap: English (US)
			main: yes
"#;

/// Every bundled-wordlist language must survive the round trip from
/// Hyprland's pretty keymap description to a BCP-47 id — a miss means
/// the engine sees an unknown current layout the moment the user
/// switches to it (empty renders, phantom re-corrections). Spanish
/// was missing until the landing page's `espa;ol` demo was first
/// exercised live.
#[test]
fn pretty_keymap_names_resolve_for_all_bundled_languages() {
    for (name, id) in [
        ("English (US)", "en-US"),
        ("Ukrainian", "uk-UA"),
        ("Russian", "ru-RU"),
        ("German", "de-DE"),
        ("French", "fr-FR"),
        ("Spanish", "es-ES"),
        ("Spanish (Latin American)", "es-ES"),
    ] {
        assert_eq!(
            keymap_to_layout(name),
            LayoutId::new(id.to_owned()),
            "{name}"
        );
    }
}

#[test]
fn normalizes_names_the_way_hyprland_prints_them() {
    assert_eq!(
        normalize_device_name("poltertype virtual keyboard"),
        "poltertype-virtual-keyboard"
    );
    assert_eq!(
        normalize_device_name("Logitech MX Keys"),
        "logitech-mx-keys"
    );
}

#[test]
fn parses_keyboard_blocks() {
    let kbs = parse_keyboards(DESYNCED);
    assert_eq!(kbs.len(), 3);
    assert_eq!(kbs[0].name, "logitech-mx-keys");
    assert_eq!(kbs[0].keymap.as_deref(), Some("English (US)"));
    assert!(!kbs[0].main);
    assert!(kbs[2].main);
}

/// The regression this module exists for: with the emitter promoted
/// to `main` and only keyd toggled to Ukrainian, the user is really
/// typing Ukrainian — `current()` must say so, not echo our own
/// emitter's stale keymap.
#[test]
fn remapper_keyboard_wins_over_main_emitter() {
    let kbs = parse_keyboards(DESYNCED);
    assert_eq!(choose_current_keymap(&kbs), Some("Ukrainian"));
}

#[test]
fn own_emitter_is_never_eligible_even_as_fallback() {
    let only_emitter = r#"Keyboards:
	Keyboard at 1:
		poltertype-virtual-keyboard
			active keymap: English (US)
			main: yes
"#;
    let kbs = parse_keyboards(only_emitter);
    assert_eq!(choose_current_keymap(&kbs), None);
}

#[test]
fn without_remapper_main_wins() {
    let no_remapper = r#"Keyboards:
	Keyboard at 1:
		logitech-mx-keys
			active keymap: English (US)
			main: no
	Keyboard at 2:
		internal-keyboard
			active keymap: Ukrainian
			main: yes
"#;
    let kbs = parse_keyboards(no_remapper);
    assert_eq!(choose_current_keymap(&kbs), Some("Ukrainian"));
}

#[test]
fn falls_back_to_first_keyboard_when_no_main_flag() {
    let no_main = r#"Keyboards:
	Keyboard at 1:
		logitech-mx-keys
			active keymap: German
"#;
    let kbs = parse_keyboards(no_main);
    assert_eq!(choose_current_keymap(&kbs), Some("German"));
}
