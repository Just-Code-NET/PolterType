//! Hand-rolled scanner for the GVariant tuple text `gdbus` prints back
//! — quote-aware splitting, so a comma or bracket inside a localised
//! display name is never read as syntax.

/// The contents of the outermost `[...]`, exclusive of the brackets.
pub(super) fn array_body(raw: &str) -> Option<&str> {
    let open = find_unquoted(raw, b'[', 0)?;
    let close = matching(raw, open, b'[', b']')?;
    raw.get(open + 1..close)
}

/// The contents of each top-level `(...)` in `body`.
pub(super) fn tuples(body: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(open) = find_unquoted(body, b'(', at) {
        let Some(close) = matching(body, open, b'(', b')') else {
            break;
        };
        if let Some(inner) = body.get(open + 1..close) {
            out.push(inner);
        }
        at = close + 1;
    }
    out
}

/// Split on `sep` where it sits outside quotes and outside nesting.
pub(super) fn split_top_level(body: &str, sep: u8) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    let mut strings = StringScan::default();
    for (i, &b) in body.as_bytes().iter().enumerate() {
        if strings.consumed(b) {
            continue;
        }
        match b {
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth = depth.saturating_sub(1),
            _ if b == sep && depth == 0 => {
                out.push(&body[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&body[start..]);
    out
}

/// Starts at byte 0 whatever `from` says — as does [`matching`]:
/// quoting state is only correct if every byte is fed to the scan, and
/// a caller resuming mid-string would read its contents as syntax.
fn find_unquoted(s: &str, needle: u8, from: usize) -> Option<usize> {
    let mut strings = StringScan::default();
    s.as_bytes()
        .iter()
        .enumerate()
        .find(|&(i, &b)| !strings.consumed(b) && i >= from && b == needle)
        .map(|(i, _)| i)
}

/// Index of the bracket closing the one at `open_at`.
fn matching(s: &str, open_at: usize, open: u8, close: u8) -> Option<usize> {
    let mut depth = 0usize;
    let mut strings = StringScan::default();
    for (i, &b) in s.as_bytes().iter().enumerate() {
        if strings.consumed(b) || i < open_at {
            continue;
        }
        if b == open {
            depth += 1;
        } else if b == close {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

/// Tracks whether the byte just seen was inside a GVariant string, so
/// the scanners above can ignore brackets and commas that a localised
/// display name happens to contain.
///
/// The delimiter has to be remembered rather than assumed: `glib`
/// prints strings in single quotes *until* the string contains one, and
/// then switches the whole literal to double quotes instead of
/// escaping. Reading that apostrophe as an opening quote desynchronises
/// the scan for the rest of the reply.
#[derive(Default)]
struct StringScan {
    delimiter: Option<u8>,
    escaped: bool,
}

impl StringScan {
    /// Feed one byte. `true` means "this byte was string content (or
    /// the quote around it) — do not read it as syntax".
    fn consumed(&mut self, b: u8) -> bool {
        match self.delimiter {
            Some(delimiter) => {
                if self.escaped {
                    self.escaped = false;
                } else if b == b'\\' {
                    self.escaped = true;
                } else if b == delimiter {
                    self.delimiter = None;
                }
                true
            }
            None if b == b'\'' || b == b'"' => {
                self.delimiter = Some(b);
                true
            }
            None => false,
        }
    }
}

/// Strip the quotes off a GVariant string field and undo its escapes.
pub(super) fn unquote(field: &str) -> Option<String> {
    let field = field.trim();
    let delimiter = field.chars().next().filter(|&c| c == '\'' || c == '"')?;
    let inner = field.strip_prefix(delimiter)?.strip_suffix(delimiter)?;

    let mut out = String::with_capacity(inner.len());
    let mut escaped = false;
    for c in inner.chars() {
        match c {
            // `\\` and the quote in use are the escapes we can meet in
            // a layout code; the C escapes come along for free rather
            // than leave a stray letter behind if one ever shows up.
            _ if escaped => {
                out.push(match c {
                    'n' => '\n',
                    't' => '\t',
                    'r' => '\r',
                    other => other,
                });
                escaped = false;
            }
            '\\' => escaped = true,
            _ => out.push(c),
        }
    }
    Some(out)
}
