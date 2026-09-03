//! Building the OS chord a clipboard const is defined in terms of.

/// `scancode` is Win SC Set-1, which for `C` and `V` coincides with
/// evdev's `KEY_C` / `KEY_V`.
pub(crate) const fn clipboard_chord(scancode: u32) -> poltertype_types::SwitchChord {
    poltertype_types::SwitchChord {
        scancode,
        ctrl: !cfg!(target_os = "macos"),
        shift: false,
        alt: false,
        meta: cfg!(target_os = "macos"),
    }
}
