//! `gdbus` invocation and assembling Cinnamon's input-source list from
//! the reply. The reply text itself is parsed by [`super::gvariant`].
//!
//! `gdbus` is glib's own D-Bus CLI and ships in the same package as
//! `gsettings`, so a Cinnamon session that has one has the other.
//! Using it keeps this crate free of a D-Bus client library and an
//! async runtime, the same trade the KDE backend makes with `qdbus`.

use super::gvariant::{array_body, split_top_level, tuples, unquote};
use super::*;
use crate::linux::shared::xkb_to_bcp47;
use crate::{LayoutError, LayoutId};
use std::process::Command;

/// One input source, reduced to the three fields we act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InputSource {
    /// Position in Cinnamon's own list — the argument
    /// `ActivateInputSourceIndex` expects, not an index into whatever
    /// we return from `list_active`.
    pub(crate) index: i32,
    pub(crate) layout: LayoutId,
    pub(crate) is_current: bool,
}

pub(crate) fn call(method: &str, args: &[&str]) -> Result<String, LayoutError> {
    let mut argv = vec![
        "call",
        "--session",
        "--dest",
        BUS_NAME,
        "--object-path",
        OBJECT_PATH,
        "--method",
        method,
    ];
    argv.extend_from_slice(args);

    let out = Command::new("gdbus")
        .args(&argv)
        .output()
        .map_err(|e| LayoutError::Os(format!("gdbus: {e}")))?;
    if !out.status.success() {
        // Worth quoting: on Cinnamon 6.4 this is how we learn the
        // method does not exist, and on a broken session it is the
        // only description of why.
        return Err(LayoutError::Os(format!(
            "gdbus {method} exited {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub(crate) fn read_sources() -> Result<Vec<InputSource>, LayoutError> {
    let mut sources = parse_input_sources(&call(GET_INPUT_SOURCES, &[])?);
    sources.sort_by_key(|s| s.index);
    Ok(sources)
}

/// Parse the reply of `GetInputSources`.
///
/// `gdbus` prints a GVariant tuple holding the single out-argument:
///
/// ```text
/// ([('xkb', 'us', 0, 'English (US)', 'en', 'us', 'us', 'us', '', '', -1, true),
///   ('xkb', 'ru', 1, 'Russian', 'ru', 'ru', 'ru', 'ru', '', '', -1, false)],)
/// ```
///
/// That needs no GVariant parser, but it does need quoting respected:
/// display names are localised, and a layout called `Hawai'ian` would
/// otherwise cut a tuple in half. Unreadable sources are skipped rather
/// than guessed at.
pub(crate) fn parse_input_sources(raw: &str) -> Vec<InputSource> {
    let Some(body) = array_body(raw) else {
        return Vec::new();
    };
    tuples(body).into_iter().filter_map(parse_source).collect()
}

fn parse_source(tuple: &str) -> Option<InputSource> {
    let fields = split_top_level(tuple, b',');
    if fields.len() != SOURCE_FIELDS {
        return None;
    }
    let layout = unquote(fields[FIELD_XKB_LAYOUT])?;
    if layout.is_empty() {
        return None;
    }
    Some(InputSource {
        index: fields[FIELD_INDEX].trim().parse().ok()?,
        layout: LayoutId::new(xkb_to_bcp47(&layout).map(str::to_owned).unwrap_or(layout)),
        is_current: fields[FIELD_IS_CURRENT].trim() == "true",
    })
}
