//! Reading a source file into the declarations the rules talk about.
//!
//! A line scanner, not a parser: `cargo fmt` is a gate here, so a
//! top-level item always starts at column 0 and nothing else does.
//! What still has to be tracked is the text that only *looks* like
//! code — this crate writes installer scripts as multi-line string
//! literals, and one of them starts a line with `fn`.

use super::consts::PLATFORM_PREDICATES;
use super::enums::Kind;
use super::types::{FileScan, Item};

/// How much of the line the scanner is still owed by an unterminated
/// string or comment when the next one starts.
#[derive(Default)]
struct Pending {
    /// Delimiter that closes an open raw string (`"##` for `r##"`).
    raw_close: Option<String>,
    block_comments: usize,
    string: bool,
}

impl Pending {
    fn inside_text(&self) -> bool {
        self.raw_close.is_some() || self.block_comments > 0 || self.string
    }
}

pub(crate) fn scan(text: &str) -> FileScan {
    let mut items = Vec::new();
    let mut nested_platform_cfg = Vec::new();
    let mut attrs: Vec<&str> = Vec::new();
    let mut pending = Pending::default();

    for (idx, line) in text.lines().enumerate() {
        let no = idx + 1;
        let carried = pending.inside_text();
        advance(line, &mut pending);
        if carried {
            continue;
        }

        let trimmed = line.trim_start();
        let indented = !line.is_empty() && line.len() != trimmed.len();

        if indented {
            if trimmed.starts_with("#[cfg") && names_platform(trimmed) {
                nested_platform_cfg.push(no);
            }
            continue;
        }

        if line.starts_with("#[") {
            attrs.push(line);
        } else if line.starts_with("//") || line.starts_with("/*") {
            // Doc comments and attributes interleave above an item.
        } else if line.is_empty() {
            attrs.clear();
        } else if let Some((kind, name, exported)) = parse_item(line) {
            items.push(Item {
                line: no,
                kind,
                name,
                exported,
                platform_cfg: attrs
                    .iter()
                    .any(|a| a.starts_with("#[cfg") && names_platform(a)),
                has_body: line.trim_end().ends_with('{'),
                path_attr: attrs.iter().any(|a| a.starts_with("#[path")),
            });
            attrs.clear();
        } else {
            attrs.clear();
        }
    }

    FileScan {
        items,
        nested_platform_cfg,
    }
}

/// Walk one line, updating what is left open at the end of it.
fn advance(line: &str, pending: &mut Pending) {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if let Some(close) = &pending.raw_close {
            if starts_with(&chars, i, close) {
                i += close.chars().count();
                pending.raw_close = None;
            } else {
                i += 1;
            }
            continue;
        }
        if pending.block_comments > 0 {
            if starts_with(&chars, i, "*/") {
                pending.block_comments -= 1;
                i += 2;
            } else if starts_with(&chars, i, "/*") {
                pending.block_comments += 1;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if pending.string {
            match chars[i] {
                '\\' => i += 2,
                '"' => {
                    pending.string = false;
                    i += 1;
                }
                _ => i += 1,
            }
            continue;
        }

        if starts_with(&chars, i, "//") {
            return;
        }
        if starts_with(&chars, i, "/*") {
            pending.block_comments = 1;
            i += 2;
            continue;
        }
        if chars[i] == '"' {
            pending.string = true;
            i += 1;
            continue;
        }
        if chars[i] == 'r' && (i == 0 || !is_ident_char(chars[i - 1])) {
            let mut j = i + 1;
            while j < chars.len() && chars[j] == '#' {
                j += 1;
            }
            if j < chars.len() && chars[j] == '"' {
                pending.raw_close = Some(format!("\"{}", "#".repeat(j - i - 1)));
                i = j + 1;
                continue;
            }
        }
        if chars[i] == '\'' {
            // A char literal closes on this line; a lifetime never opens.
            let escaped = chars.get(i + 1) == Some(&'\\');
            let plain = chars.get(i + 2) == Some(&'\'');
            if escaped || plain {
                let mut j = i + 1;
                while j < chars.len() && chars[j] != '\'' {
                    j += if chars[j] == '\\' { 2 } else { 1 };
                }
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
}

fn starts_with(chars: &[char], at: usize, needle: &str) -> bool {
    needle
        .chars()
        .enumerate()
        .all(|(k, c)| chars.get(at + k) == Some(&c))
}

/// Does this `cfg` attribute name a platform rather than a feature?
pub(crate) fn names_platform(attr: &str) -> bool {
    PLATFORM_PREDICATES.iter().any(|p| contains_word(attr, p))
}

/// `needle` as a bare predicate, so `feature = "windows"` does not read
/// as the `windows` one.
fn contains_word(haystack: &str, needle: &str) -> bool {
    let bytes = haystack.as_bytes();
    let mut from = 0;
    while let Some(at) = haystack[from..].find(needle) {
        let start = from + at;
        let end = start + needle.len();
        let before = start.checked_sub(1).map(|i| bytes[i]);
        let after = bytes.get(end).copied();
        let bounded = !before.is_some_and(is_ident_byte) && !after.is_some_and(is_ident_byte);
        if bounded && before != Some(b'"') {
            return true;
        }
        from = end;
    }
    false
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// The kind, name and visibility of a top-level declaration.
fn parse_item(line: &str) -> Option<(Kind, String, bool)> {
    let (rest, exported) = strip_visibility(line);
    let mut toks = rest.split_whitespace().peekable();
    // `impl<T> Trait for Foo` attaches its generics to the keyword.
    let mut keyword = keyword_of(toks.next()?);
    loop {
        match keyword {
            "default" | "async" | "unsafe" => keyword = keyword_of(toks.next()?),
            "extern" => {
                let next = toks.next()?;
                keyword = keyword_of(if next.starts_with('"') {
                    toks.next()?
                } else {
                    next
                });
            }
            _ => break,
        }
    }

    let kind = match keyword {
        "const" => {
            if toks.peek() == Some(&"fn") {
                toks.next();
                Kind::Fn
            } else {
                Kind::Const
            }
        }
        "static" => {
            if toks.peek() == Some(&"mut") {
                toks.next();
            }
            Kind::Static
        }
        "type" => Kind::TypeAlias,
        "enum" => Kind::Enum,
        "struct" => Kind::Struct,
        "union" => Kind::Union,
        "trait" => Kind::Trait,
        "fn" => Kind::Fn,
        "impl" => return Some((Kind::Impl, String::new(), exported)),
        "mod" => Kind::Mod,
        "use" => return Some((Kind::Use, String::new(), exported)),
        _ if keyword.starts_with("macro_rules!") => {
            let name = keyword
                .strip_prefix("macro_rules!")
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .or_else(|| toks.next().map(str::to_owned))
                .unwrap_or_default();
            return Some((Kind::Macro, identifier(&name), exported));
        }
        _ => return None,
    };

    let name = toks.next().map(identifier).unwrap_or_default();
    if name.is_empty() {
        return None;
    }
    Some((kind, name, exported))
}

/// Split a leading `pub`, `pub(crate)` or `pub(in path)` off the line.
fn strip_visibility(line: &str) -> (&str, bool) {
    let Some(rest) = line.strip_prefix("pub") else {
        return (line, false);
    };
    if let Some(open) = rest.strip_prefix('(') {
        return match open.find(')') {
            Some(at) => (open[at + 1..].trim_start(), true),
            None => (line, false),
        };
    }
    match rest.strip_prefix(char::is_whitespace) {
        Some(rest) => (rest.trim_start(), true),
        None => (line, false),
    }
}

/// The leading keyword of a token, without whatever is glued to it.
fn keyword_of(token: &str) -> &str {
    let end = token
        .find(|c: char| !(c.is_ascii_alphabetic() || c == '_' || c == '!'))
        .unwrap_or(token.len());
    &token[..end]
}

/// The identifier at the head of a token: `Foo<T>` → `Foo`, `NAME:` →
/// `NAME`, `f(` → `f`.
fn identifier(token: &str) -> String {
    token
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect()
}
