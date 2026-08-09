//! Which language to show the interface in.

/// Resolve the UI locale: an explicit setting wins, otherwise the
/// environment, otherwise English.
///
/// `"auto"`, `"system"` and the empty string all mean "ask the
/// environment". `"system"` is what `[general].ui_language` has shipped
/// with since the setting was added, so honouring it costs nothing and
/// beats silently rewriting every existing config.
///
/// Detection is the POSIX trio (`LC_ALL`, `LC_MESSAGES`, `LANG`) in the
/// order the C library uses. Windows sets none of these and lands on
/// English unless the user picks a language — deliberate, since reading
/// the Windows locale means a `#[cfg(target_os)]` in a crate that holds
/// none, and a picker beats a guess anyway.
pub fn resolve_locale(requested: Option<&str>) -> String {
    if let Some(explicit) = requested {
        let trimmed = explicit.trim();
        let automatic =
            trimmed.eq_ignore_ascii_case("auto") || trimmed.eq_ignore_ascii_case("system");
        if !trimmed.is_empty() && !automatic {
            return normalise(trimmed);
        }
    }
    for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Some(raw) = std::env::var_os(var) {
            let value = raw.to_string_lossy();
            let trimmed = value.trim();
            // `C` and `POSIX` are "no locale", not a language.
            if trimmed.is_empty() || trimmed == "C" || trimmed == "POSIX" {
                continue;
            }
            return normalise(trimmed);
        }
    }
    "en".to_owned()
}

/// `uk_UA.UTF-8` → `uk_UA`; lowercase the language subtag so lookups
/// are predictable. The encoding and any `@modifier` are dropped —
/// they say nothing about which words to show.
fn normalise(locale: &str) -> String {
    let without_encoding = locale.split(['.', '@']).next().unwrap_or(locale);
    let mut parts = without_encoding.splitn(2, ['_', '-']);
    let language = parts.next().unwrap_or("").to_ascii_lowercase();
    match parts.next() {
        Some(region) if !region.is_empty() => format!("{language}_{}", region.to_ascii_uppercase()),
        _ => language,
    }
}
