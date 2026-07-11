//! Focus-exe → active profile resolution.

use super::*;

/// Pick the active profile id for a focused app's exe basename,
/// or `None` to fall through to the global overlay. Resolution
/// order:
///
/// 1. First profile whose `apps` list contains a case-insensitive
///    match for `focused_basename` wins.
/// 2. Otherwise, [`WordlistSettings::default_profile`] if it names
///    a known profile.
/// 3. Otherwise, `None` (caller uses the global overlay only).
///
/// `focused_basename` should already be just the basename — the
/// caller usually has it after `Path::new(exe).file_name()`. We
/// don't strip a path here because the function is also called
/// from tests with synthetic data.
pub fn resolve_active_profile<'a>(
    settings: &'a WordlistSettings,
    focused_basename: Option<&str>,
) -> Option<&'a str> {
    if let Some(name) = focused_basename {
        for p in &settings.profiles {
            if p.apps.iter().any(|a| a.eq_ignore_ascii_case(name)) {
                return Some(&p.id);
            }
        }
    }
    if !settings.default_profile.is_empty()
        && settings
            .profiles
            .iter()
            .any(|p| p.id == settings.default_profile)
    {
        return Some(&settings.default_profile);
    }
    None
}
