//! Windows and macOS identify a window's application by the binary it
//! came from, so there is nothing to declare or install here.

/// Windows and macOS identify a window's application by the binary it
/// came from, so there is nothing to declare.
pub fn window_platform_specific() -> iced::window::settings::PlatformSpecific {
    iced::window::settings::PlatformSpecific::default()
}

/// Nothing to install: the executable already carries its own identity.
pub fn install_desktop_entry() {}
