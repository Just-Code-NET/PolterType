//! Asking fontconfig, which is what every other application on the
//! desktop asks too.

/// fontconfig's answer for the sans-serif family, so the app looks
/// native. Costs one short-lived process, once, when a window opens.
#[must_use]
pub fn ui_font_family() -> Option<String> {
    let out = std::process::Command::new("fc-match")
        .args(["-f", "%{family[0]}", "sans-serif"])
        .output()
        .ok()?;
    let name = String::from_utf8(out.stdout).ok()?.trim().to_owned();
    (out.status.success() && !name.is_empty()).then_some(name)
}
