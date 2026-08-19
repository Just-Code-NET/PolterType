//! `Aff` — parsed .aff affix table and word expansion.

use super::*;
use anyhow::{Result, bail};
use std::collections::{HashMap, HashSet};

/// Parsed Hunspell `.aff` file — just enough to drive `expand`.
#[derive(Debug)]
pub struct Aff {
    flag_type: FlagType,
    sfx: HashMap<String, AffixGroup>,
    pfx: HashMap<String, AffixGroup>,
}

impl Aff {
    /// Parse the textual contents of a `.aff` file.
    pub fn parse(text: &str) -> Result<Self> {
        let mut flag_type = FlagType::Ascii;
        let mut sfx: HashMap<String, AffixGroup> = HashMap::new();
        let mut pfx: HashMap<String, AffixGroup> = HashMap::new();

        // Rule lines accumulate here until the next block header or
        // EOF flushes them into `sfx` / `pfx`.
        let mut current: Option<(bool, String, AffixGroup)> = None;

        for raw in text.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }

            // FLAG <type> must be honoured before any rule line is
            // parsed, since parsing a rule reads flag strings.
            if let Some(rest) = line.strip_prefix("FLAG ") {
                flag_type = match rest.trim() {
                    "long" => FlagType::Long,
                    "UTF-8" => FlagType::Utf8,
                    "num" => FlagType::Num,
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

        // BFS over (form, flags-still-to-apply). The depth cap is a
        // safety net against circular continuation flags in third-party
        // dictionaries.
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
        split_flags(s, self.flag_type)
    }
}
