use super::*;
use crate::LayoutId;

/// Verbatim `swaymsg -t get_inputs` from the desktop-matrix guest,
/// sway 1.11 on Ubuntu 26.04, 2026-08-24 — including the power
/// button and video bus entries that also call themselves keyboards
/// and carry one layout each.
const REAL: &str = r#"[
  {
    "identifier": "2:6:ImExPS\/2_Generic_Explorer_Mouse",
    "name": "ImExPS\/2 Generic Explorer Mouse",
    "type": "pointer",
    "vendor": 2,
    "product": 6
  },
  {
    "identifier": "0:1:Power_Button",
    "name": "Power Button",
    "type": "keyboard",
    "xkb_layout_names": [
      "English (US)"
    ],
    "xkb_active_layout_index": 0,
    "xkb_active_layout_name": "English (US)",
    "vendor": 0,
    "product": 1
  },
  {
    "identifier": "1:1:AT_Translated_Set_2_keyboard",
    "name": "AT Translated Set 2 keyboard",
    "type": "keyboard",
    "repeat_delay": 600,
    "repeat_rate": 25,
    "xkb_layout_names": [
      "English (US)",
      "Russian"
    ],
    "xkb_active_layout_index": 1,
    "xkb_active_layout_name": "Russian",
    "libinput": {
      "send_events": "enabled"
    },
    "vendor": 1,
    "product": 1
  }
]"#;

#[test]
fn the_keyboard_with_layouts_wins_over_the_power_button() {
    let kb = parse_inputs(REAL);
    assert_eq!(
        kb.layouts,
        vec![LayoutId::from("en-US"), LayoutId::from("ru-RU")]
    );
    assert_eq!(kb.active, Some(1));
}

#[test]
fn a_session_with_one_layout_reports_one() {
    let one = REAL.replace(
        "      \"English (US)\",\n      \"Russian\"\n",
        "      \"English (US)\"\n",
    );
    let kb = parse_inputs(&one);
    assert_eq!(kb.layouts, vec![LayoutId::from("en-US")]);
}

#[test]
fn no_keyboard_at_all_is_empty_rather_than_a_guess() {
    let kb = parse_inputs("[\n  {\n    \"type\": \"pointer\"\n  }\n]");
    assert!(kb.layouts.is_empty());
    assert_eq!(kb.active, None);
}

#[test]
fn sways_escaped_slashes_survive() {
    assert_eq!(
        json_string(r#"    "name": "ImExPS\/2 Generic Explorer Mouse","#).as_deref(),
        Some("ImExPS/2 Generic Explorer Mouse")
    );
}
