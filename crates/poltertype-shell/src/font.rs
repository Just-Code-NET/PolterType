//! Which font family the app's own windows are drawn in.

/// A font family this machine really has, for the Settings window and
/// the suggestion tooltip.
///
/// Both draw through `cosmic-text`, and both used to ask for a family
/// nobody guarantees: iced's `Font::DEFAULT` is the *name* "Fira Sans",
/// and cosmic-text resolves the generic `SansSerif` to that same name.
/// Where the machine has no Fira Sans — which is most of them; neither
/// this project's development laptop nor a stock Ubuntu 26.04 desktop
/// has it — the request falls through to whichever of the hundreds of
/// installed faces the font database answers with, and that is not a
/// decision anyone made. On Ubuntu it answered with a face carrying no
/// text glyphs: every label that had not asked for a font by name came
/// out blank, and the Settings window rendered its headers, its layout
/// ids, and nothing else. Measured in the desktop-matrix guest,
/// 2026-08-27.
///
/// `None` means "no better idea than the default" — the caller keeps
/// whatever it was going to use.
#[must_use]
pub fn ui_font_family() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        // fontconfig is what every other application on the desktop
        // asks, so its answer is the one that will look native. Asking
        // it costs one short-lived process, once, when a window opens.
        let out = std::process::Command::new("fc-match")
            .args(["-f", "%{family[0]}", "sans-serif"])
            .output()
            .ok()?;
        let name = String::from_utf8(out.stdout).ok()?.trim().to_owned();
        (out.status.success() && !name.is_empty()).then_some(name)
    }
    #[cfg(target_os = "windows")]
    {
        // The Windows UI font since Vista, present on every supported
        // version.
        Some("Segoe UI".to_owned())
    }
    #[cfg(target_os = "macos")]
    {
        // Not the system UI font (`.AppleSystemUIFont` is not a family
        // a font database can look up), but the one macOS has shipped
        // under a real name for a decade.
        Some("Helvetica Neue".to_owned())
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        None
    }
}
