//! Affix rule parsing and application.

use super::*;

/// Split the rule's `<add>` field into its `(add, continuation_flags)`
/// halves. The `/`-suffix syntax (`"ed/Y"`, `"ing/MX"`) means "after
/// adding `ed`, also recursively apply rules tagged with flags `Y`,
/// then recursively apply with `M` and `X`." We respect the parent
/// `.aff`'s flag mode when chunking the continuation flags.
pub(crate) fn parse_add(part: &str, flag_type: FlagType) -> (String, Vec<String>) {
    let (add_raw, cont_raw) = match part.split_once('/') {
        Some((a, c)) => (a, c),
        None => (part, ""),
    };
    let add = if add_raw == "0" {
        String::new()
    } else {
        add_raw.to_string()
    };
    let continuation: Vec<String> = if cont_raw.is_empty() {
        Vec::new()
    } else {
        match flag_type {
            FlagType::Ascii | FlagType::Utf8 => cont_raw.chars().map(|c| c.to_string()).collect(),
            FlagType::Long => {
                let chars: Vec<char> = cont_raw.chars().collect();
                chars.chunks(2).map(|c| c.iter().collect()).collect()
            }
        }
    };
    (add, continuation)
}

/// Compile a Hunspell condition string like `"[аяіе]яти"` /
/// `"[^xyz]"` / `"."` into a sequence of atoms.
pub(crate) fn parse_condition(s: &str) -> (Vec<CondAtom>, bool) {
    if s == "." {
        return (Vec::new(), true);
    }
    let mut atoms = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '.' => atoms.push(CondAtom::Any),
            '[' => {
                let neg = chars.peek() == Some(&'^');
                if neg {
                    chars.next();
                }
                let mut class = Vec::new();
                for cc in chars.by_ref() {
                    if cc == ']' {
                        break;
                    }
                    class.push(cc);
                }
                if neg {
                    atoms.push(CondAtom::NegClass(class));
                } else {
                    atoms.push(CondAtom::Class(class));
                }
            }
            other => atoms.push(CondAtom::Char(other)),
        }
    }
    (atoms, false)
}

pub(crate) fn apply_sfx(word: &str, rule: &AffixRule) -> Option<String> {
    let chars: Vec<char> = word.chars().collect();

    // Match condition at the END of the word.
    if !rule.unconditional {
        if chars.len() < rule.condition.len() {
            return None;
        }
        let start = chars.len() - rule.condition.len();
        for (i, atom) in rule.condition.iter().enumerate() {
            if !match_atom(atom, chars[start + i]) {
                return None;
            }
        }
    }

    if chars.len() < rule.strip_chars {
        return None;
    }
    let kept: String = chars[..chars.len() - rule.strip_chars].iter().collect();
    Some(format!("{kept}{}", rule.add))
}

pub(crate) fn apply_pfx(word: &str, rule: &AffixRule) -> Option<String> {
    let chars: Vec<char> = word.chars().collect();

    if !rule.unconditional {
        if chars.len() < rule.condition.len() {
            return None;
        }
        for (i, atom) in rule.condition.iter().enumerate() {
            if !match_atom(atom, chars[i]) {
                return None;
            }
        }
    }

    if chars.len() < rule.strip_chars {
        return None;
    }
    let kept: String = chars[rule.strip_chars..].iter().collect();
    Some(format!("{}{kept}", rule.add))
}

pub(crate) fn match_atom(atom: &CondAtom, ch: char) -> bool {
    match atom {
        CondAtom::Any => true,
        CondAtom::Char(c) => *c == ch,
        CondAtom::Class(chars) => chars.contains(&ch),
        CondAtom::NegClass(chars) => !chars.contains(&ch),
    }
}
