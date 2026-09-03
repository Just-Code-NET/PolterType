//! Reading a source file into the declarations the rules talk about.
//!
//! A line scanner, not a parser: `cargo fmt` is a gate here, so a
//! top-level item always starts at column 0 and nothing else does.
//! Which columns are code at all is [`super::text`]'s job.

use super::consts::PLATFORM_PREDICATES;
use super::enums::Kind;
use super::text::{Pending, advance};
use super::types::{FileScan, Item};

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
        } else if let Some((kind, name, exported, inherent_impl)) = parse_item(line) {
            items.push(Item {
                line: no,
                kind,
                name,
                exported,
                inherent_impl,
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
        lines: text.lines().count(),
    }
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

/// The kind, name and visibility of a top-level declaration, plus
/// whether an `impl` is inherent rather than a trait implementation.
fn parse_item(line: &str) -> Option<(Kind, String, bool, bool)> {
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
        "impl" => {
            let (target, inherent) = impl_target(rest);
            return Some((Kind::Impl, target, exported, inherent));
        }
        "mod" => Kind::Mod,
        "use" => return Some((Kind::Use, String::new(), exported, false)),
        _ if keyword.starts_with("macro_rules!") => {
            let name = keyword
                .strip_prefix("macro_rules!")
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .or_else(|| toks.next().map(str::to_owned))
                .unwrap_or_default();
            return Some((Kind::Macro, identifier(&name), exported, false));
        }
        _ => return None,
    };

    let name = toks.next().map(identifier).unwrap_or_default();
    if name.is_empty() {
        return None;
    }
    Some((kind, name, exported, false))
}

/// The type an `impl` block is written for, and whether it is the
/// type's own — `impl Foo`, not `impl Trait for Foo`. Generics are
/// skipped by matching angle brackets rather than by splitting on
/// whitespace: `impl<T: Debug + Clone> Foo<T>` has spaces inside them.
fn impl_target(rest: &str) -> (String, bool) {
    let Some(at) = rest.find("impl") else {
        return (String::new(), false);
    };
    let after = skip_generics(rest[at + "impl".len()..].trim_start());
    let head = after
        .split(" where ")
        .next()
        .unwrap_or(after)
        .split('{')
        .next()
        .unwrap_or(after);
    match head.split_once(" for ") {
        Some((_, target)) => (type_head(target), false),
        None => (type_head(head), true),
    }
}

/// Everything after a leading `<…>`, brackets balanced.
fn skip_generics(s: &str) -> &str {
    if !s.starts_with('<') {
        return s;
    }
    let mut depth = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth == 0 {
                    return s[i + c.len_utf8()..].trim_start();
                }
            }
            _ => {}
        }
    }
    s
}

/// The bare name of an impl target: `crate::a::Foo<T>` → `Foo`.
fn type_head(s: &str) -> String {
    let s = s.trim().trim_start_matches(['&', '*']).trim();
    let s = s.split('<').next().unwrap_or(s);
    identifier(s.rsplit("::").next().unwrap_or(s))
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
