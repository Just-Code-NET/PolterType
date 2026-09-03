//! Telling code from the text that only looks like it.
//!
//! The scanner reads line by line, so it has to carry what a string or
//! a comment left open across the newline. This crate writes installer
//! scripts as multi-line literals and one of them starts a line with
//! `fn`; without this, everything after it reads as a declaration.

/// How much of the line the scanner is still owed by an unterminated
/// string or comment when the next one starts.
#[derive(Default)]
pub(crate) struct Pending {
    /// Delimiter that closes an open raw string (`"##` for `r##"`).
    raw_close: Option<String>,
    block_comments: usize,
    string: bool,
}

impl Pending {
    pub(crate) fn inside_text(&self) -> bool {
        self.raw_close.is_some() || self.block_comments > 0 || self.string
    }
}

/// Walk one line, updating what is left open at the end of it.
pub(crate) fn advance(line: &str, pending: &mut Pending) {
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

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}
