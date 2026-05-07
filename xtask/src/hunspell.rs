//! Tiny Hunspell `.aff` parser + `.dic` expander.
//!
//! ## Why this exists
//!
//! Hunspell dictionaries store **stems**, not surface forms. The
//! `.dic` file lists each stem with a flag string (`мати/Z{P`,
//! `find/SDG`); the matching `.aff` file contains affix rules
//! per-flag that expand the stem into all inflected forms (`має`,
//! `матиму`, `finds`, `finding`, `found`, …). Without an expander,
//! ~70 % of the verbal vocabulary in any inflected language is
//! missing from the surface FST — exactly the gap that produced the
//! "має" auto-delete bug (DECISIONS.md, 2026-05-07).
//!
//! ## What it covers
//!
//! Enough Hunspell to expand the LibreOffice dictionaries we ship —
//! that means:
//!
//! * Suffix (`SFX`) and prefix (`PFX`) rules with simple
//!   `<strip> <add> <condition>` shape.
//! * Condition patterns: literal chars, `.` (any single char), `[abc]`
//!   class, `[^abc]` negative class.
//! * Three flag-encoding modes: default ASCII (one flag per char),
//!   `FLAG long` (two chars per flag), and `FLAG UTF-8` (one Unicode
//!   char per flag — same shape as ASCII at the parser level).
//! * Continuation flags inside `<add>/CONT` — recursively expanded
//!   with a depth cap to keep pathological cases bounded.
//!
//! ## What it does NOT cover
//!
//! * `FLAG num` (comma-separated decimal flags) — none of our
//!   dictionaries use it; we error out if encountered so the build
//!   fails loudly rather than silently mis-expanding.
//! * `COMPOUND*` rules (compound-word generation). Compounds are a
//!   tiny fraction of inflected forms in our target languages and
//!   the engine doesn't need them — wrong-layout detection works
//!   on individual word boundaries, never on multi-stem compounds.
//! * Cross-product PFX × SFX combinations. uk_UA has zero PFX rules,
//!   the others have a handful — generating each one's PFX-only and
//!   SFX-only forms is enough vocabulary in practice. We skip the
//!   cross to keep the expander small and the FST size bounded.
//! * `ICONV` / `OCONV` input/output character conversions. Those
//!   are spell-checker concerns (normalising user input before
//!   lookup); we generate canonical forms only.
//! * `MAP`, `REP`, `BREAK`, `KEY`, `TRY`, `WORDCHARS`, `IGNORE`,
//!   `NEEDAFFIX`, `CIRCUMFIX`, `ONLYINCOMPOUND`, …  — also
//!   spell-checker-side. We ignore them while parsing.
//!
//! Trade-off: this is a **lossy** Hunspell port — the resulting FST
//! covers most surface vocabulary the user will type in prose, but
//! not the corner cases (compound nouns in German that aren't listed
//! as separate stems, deep cross-product chains, etc.). The
//! `data/wordlists/<lang>-extras.txt` overlay is the escape hatch
//! when a missing form bites.

use std::collections::{HashMap, HashSet};

use anyhow::{Result, bail};

/// Parsed Hunspell `.aff` file — just enough to drive `expand`.
#[derive(Debug)]
pub struct Aff {
    flag_type: FlagType,
    sfx: HashMap<String, AffixGroup>,
    pfx: HashMap<String, AffixGroup>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FlagType {
    /// Default Hunspell mode — one ASCII char per flag.
    Ascii,
    /// `FLAG long` — two chars per flag, packed.
    Long,
    /// `FLAG UTF-8` — one Unicode char per flag (used by es_ES).
    Utf8,
}

#[derive(Debug)]
struct AffixGroup {
    /// `Y` in the block header — controls PFX × SFX combination.
    /// We don't generate cross-products today; this is parsed for
    /// fidelity / future use.
    #[allow(dead_code)]
    cross_product: bool,
    rules: Vec<AffixRule>,
}

#[derive(Debug)]
struct AffixRule {
    /// Number of *characters* (not bytes) to strip from the relevant
    /// end of the word. `0` if the rule's strip field is the literal
    /// string `"0"`.
    strip_chars: usize,
    /// Characters to append (SFX) or prepend (PFX). Empty string if
    /// the rule's add field is `"0"`.
    add: String,
    /// Continuation flags from the `<add>/<flags>` syntax — every
    /// rule under each of these flags is also applied to this rule's
    /// output. Most of our dictionaries don't use this.
    continuation: Vec<String>,
    /// Condition atoms matched against the word's relevant end
    /// (suffix end for SFX, prefix start for PFX).
    condition: Vec<CondAtom>,
    /// `true` if the source condition is the literal `.` — match any
    /// word. We track this so a `.`-condition with zero atoms doesn't
    /// look like a length-0 condition that always matches.
    unconditional: bool,
}

/// One atom of a Hunspell condition pattern.
#[derive(Debug)]
enum CondAtom {
    /// `.` — matches any single character.
    Any,
    /// Literal character — must match exactly.
    Char(char),
    /// `[abc]` — character must be one of these.
    Class(Vec<char>),
    /// `[^abc]` — character must NOT be one of these.
    NegClass(Vec<char>),
}

impl Aff {
    /// Parse the textual contents of a `.aff` file.
    pub fn parse(text: &str) -> Result<Self> {
        let mut flag_type = FlagType::Ascii;
        let mut sfx: HashMap<String, AffixGroup> = HashMap::new();
        let mut pfx: HashMap<String, AffixGroup> = HashMap::new();

        // While we're inside an SFX/PFX block, append rule lines into
        // this slot; when we hit the next block header (or EOF), flush
        // it into `sfx` / `pfx`.
        let mut current: Option<(bool, String, AffixGroup)> = None;

        for raw in text.lines() {
            // Strip inline `#` comments and surrounding whitespace.
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }

            // FLAG <type> — must be honoured before any rule line is
            // parsed, since the parser reads flag strings.
            if let Some(rest) = line.strip_prefix("FLAG ") {
                flag_type = match rest.trim() {
                    "long" => FlagType::Long,
                    "UTF-8" => FlagType::Utf8,
                    "num" => bail!(
                        "this expander does not support `FLAG num` dictionaries — \
                         the affected dictionary would have to be re-encoded with \
                         `FLAG long` or default ASCII flags first"
                    ),
                    other => bail!("unknown FLAG mode `{other}`"),
                };
                continue;
            }

            // SFX / PFX line — could be a block header or a rule.
            let is_sfx = line.starts_with("SFX ");
            let is_pfx = line.starts_with("PFX ");
            if !(is_sfx || is_pfx) {
                continue; // SET, TRY, MAP, ICONV, OCONV, BREAK, … — ignored.
            }
            let rest = &line[4..]; // skip "SFX " / "PFX "
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() < 2 {
                bail!(
                    "malformed {} line: `{line}`",
                    if is_sfx { "SFX" } else { "PFX" }
                );
            }
            let flag = parts[0].to_string();

            // Block header: `<flag> Y N <count>` or `<flag> N N <count>`.
            if parts.len() >= 3 && (parts[1] == "Y" || parts[1] == "N") {
                let cross_product = parts[1] == "Y";
                let group = AffixGroup {
                    cross_product,
                    rules: Vec::new(),
                };
                // Flush the previous block before opening a new one.
                if let Some((was_sfx, prev_flag, prev_group)) = current.take() {
                    let target = if was_sfx { &mut sfx } else { &mut pfx };
                    target.insert(prev_flag, prev_group);
                }
                current = Some((is_sfx, flag, group));
                continue;
            }

            // Rule body: `<flag> <strip> <add> <condition> [...]`.
            if parts.len() < 4 {
                bail!(
                    "malformed {} rule: `{line}`",
                    if is_sfx { "SFX" } else { "PFX" }
                );
            }
            let strip_part = parts[1];
            let add_part = parts[2];
            let cond_part = parts[3];

            let strip_chars = if strip_part == "0" {
                0
            } else {
                strip_part.chars().count()
            };
            let (add, continuation) = parse_add(add_part, flag_type);
            let (condition, unconditional) = parse_condition(cond_part);

            let rule = AffixRule {
                strip_chars,
                add,
                continuation,
                condition,
                unconditional,
            };

            match current.as_mut() {
                Some((was_sfx, _, group)) if *was_sfx == is_sfx => group.rules.push(rule),
                Some((_, _, _)) => {
                    bail!("rule kind switched mid-block at `{line}` — likely a malformed .aff")
                }
                None => bail!("rule outside any block: `{line}`"),
            }
        }

        // Flush trailing block.
        if let Some((was_sfx, prev_flag, prev_group)) = current.take() {
            let target = if was_sfx { &mut sfx } else { &mut pfx };
            target.insert(prev_flag, prev_group);
        }

        Ok(Self {
            flag_type,
            sfx,
            pfx,
        })
    }

    /// Expand a single `<stem>/<flags>` entry into the set of surface
    /// forms (including the bare stem). `flags_str` may be empty —
    /// in which case only the stem itself is returned.
    pub fn expand(&self, stem: &str, flags_str: &str) -> HashSet<String> {
        let mut out: HashSet<String> = HashSet::new();
        out.insert(stem.to_string());

        // BFS over (form, flags-still-to-apply) — each rule with
        // continuation pushes a new (form, cont_flags) frontier.
        // Capped depth is a safety net against pathological circular
        // continuation flags in third-party dictionaries.
        const MAX_DEPTH: usize = 4;
        let mut frontier: Vec<(String, Vec<String>, usize)> =
            vec![(stem.to_string(), self.parse_flags(flags_str), 0)];

        while let Some((word, flags, depth)) = frontier.pop() {
            for flag in &flags {
                if let Some(group) = self.sfx.get(flag) {
                    for rule in &group.rules {
                        if let Some(form) = apply_sfx(&word, rule)
                            && out.insert(form.clone())
                            && !rule.continuation.is_empty()
                            && depth < MAX_DEPTH
                        {
                            frontier.push((form, rule.continuation.clone(), depth + 1));
                        }
                    }
                }
                if let Some(group) = self.pfx.get(flag) {
                    for rule in &group.rules {
                        if let Some(form) = apply_pfx(&word, rule)
                            && out.insert(form.clone())
                            && !rule.continuation.is_empty()
                            && depth < MAX_DEPTH
                        {
                            frontier.push((form, rule.continuation.clone(), depth + 1));
                        }
                    }
                }
            }
        }

        out
    }

    fn parse_flags(&self, s: &str) -> Vec<String> {
        match self.flag_type {
            FlagType::Ascii | FlagType::Utf8 => s.chars().map(|c| c.to_string()).collect(),
            FlagType::Long => {
                let chars: Vec<char> = s.chars().collect();
                chars.chunks(2).map(|c| c.iter().collect()).collect()
            }
        }
    }
}

/// Split the rule's `<add>` field into its `(add, continuation_flags)`
/// halves. The `/`-suffix syntax (`"ed/Y"`, `"ing/MX"`) means "after
/// adding `ed`, also recursively apply rules tagged with flags `Y`,
/// then recursively apply with `M` and `X`." We respect the parent
/// `.aff`'s flag mode when chunking the continuation flags.
fn parse_add(part: &str, flag_type: FlagType) -> (String, Vec<String>) {
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
fn parse_condition(s: &str) -> (Vec<CondAtom>, bool) {
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

fn apply_sfx(word: &str, rule: &AffixRule) -> Option<String> {
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

fn apply_pfx(word: &str, rule: &AffixRule) -> Option<String> {
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

fn match_atom(atom: &CondAtom, ch: char) -> bool {
    match atom {
        CondAtom::Any => true,
        CondAtom::Char(c) => *c == ch,
        CondAtom::Class(chars) => chars.contains(&ch),
        CondAtom::NegClass(chars) => !chars.contains(&ch),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal hand-rolled `.aff` covering the key features used by
    /// the LibreOffice dictionaries we ingest. Verifies the parser
    /// understands FLAG modes, both kinds of conditions, and the
    /// `<add>/<continuation>` syntax — without depending on a 6 KB
    /// upstream file the tests would have to vendor.
    fn aff(s: &str) -> Aff {
        Aff::parse(s).expect("parse")
    }

    #[test]
    fn expands_simple_suffix_with_class_condition() {
        // The exact rule that generates `має` from `мати/Z`.
        let a = aff("SET UTF-8\n\
             SFX Z Y 1\n\
             SFX Z ти є [аіуяї]ти\n");
        let forms = a.expand("мати", "Z");
        assert!(forms.contains("мати"), "stem must be present");
        assert!(forms.contains("має"), "expected `має` in {forms:?}");
    }

    #[test]
    fn expands_unconditional_dot_rule() {
        let a = aff("PFX X Y 1\nPFX X 0 не .\n");
        let forms = a.expand("має", "X");
        assert!(forms.contains("немає"), "expected `немає` in {forms:?}");
    }

    #[test]
    fn negative_class_skips_non_matches() {
        // SFX condition `[^аеи]` — only words ending in NOT a/e/и get
        // the suffix.
        let a = aff("SFX Q Y 1\n\
             SFX Q 0 z [^aei]\n");
        let f1 = a.expand("dog", "Q");
        assert!(f1.contains("dogz"), "[^aei] should match `g`");
        let f2 = a.expand("dia", "Q"); // ends in `a` — class-excluded
        assert_eq!(f2.len(), 1, "no expansion expected, got {f2:?}");
    }

    #[test]
    fn long_flags_chunk_in_pairs() {
        let a = aff("FLAG long\n\
             SFX AB Y 1\n\
             SFX AB 0 s .\n\
             SFX CD Y 1\n\
             SFX CD 0 ed .\n");
        // Flags string `ABCD` = two flags `AB` and `CD`.
        let forms = a.expand("walk", "ABCD");
        assert!(forms.contains("walks"), "expected `walks`");
        assert!(forms.contains("walked"), "expected `walked`");
    }

    #[test]
    fn ignores_unknown_directives() {
        let a = aff("SET UTF-8\n\
             TRY abcde\n\
             MAP 1\n\
             MAP eé\n\
             ICONV ʼ '\n\
             BREAK 1\n\
             BREAK -\n\
             SFX A Y 1\n\
             SFX A 0 s .\n");
        assert!(a.expand("cat", "A").contains("cats"));
    }

    #[test]
    fn continuation_flag_recurses() {
        let a = aff("SFX A Y 1\n\
             SFX A 0 ed/B .\n\
             SFX B Y 1\n\
             SFX B 0 ly .\n");
        let forms = a.expand("walk", "A");
        assert!(forms.contains("walked"), "first-stage SFX A");
        assert!(forms.contains("walkedly"), "B applied to walked");
    }

    #[test]
    fn rule_strips_correctly_in_unicode() {
        // `жити` (live, infinitive) under uk-UA.aff has a Z rule
        // `SFX Z ти веш жити` → `жити` ⇒ `живеш`.
        let a = aff("SFX Z Y 1\nSFX Z ти веш жити\n");
        let forms = a.expand("жити", "Z");
        assert!(forms.contains("живеш"), "got {forms:?}");
    }

    #[test]
    fn flag_num_is_rejected() {
        let err = Aff::parse("FLAG num\n").expect_err("FLAG num should fail");
        assert!(
            err.to_string().contains("FLAG num"),
            "error mentions FLAG num: {err}"
        );
    }
}
