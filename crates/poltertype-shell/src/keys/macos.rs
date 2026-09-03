//! macOS's key glyphs and its update-permission caveat.

pub fn key_glyph(token: &str) -> Option<&'static str> {
    Some(match token.to_ascii_lowercase().as_str() {
        "ctrl" | "control" => "⌃",
        "alt" | "option" => "⌥",
        "shift" => "⇧",
        "meta" | "cmd" | "command" | "super" | "win" => "⌘",
        "backspace" => "⌫",
        "enter" | "return" => "↩",
        "tab" => "⇥",
        "esc" | "escape" => "⎋",
        "space" => "Space",
        "up" => "↑",
        "down" => "↓",
        "left" => "←",
        "right" => "→",
        _ => return None,
    })
}

pub fn update_permission_note() -> Option<&'static str> {
    Some(
        "After updating, macOS will ask for Accessibility and Input Monitoring again: \
         these builds are not signed with an Apple Developer ID, so a permission is tied \
         to the exact copy of the app and updating replaces it. The Setup pane will say \
         so, and the fix is to remove PolterType from each list and add it back.",
    )
}
