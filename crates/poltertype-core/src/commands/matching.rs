//! Trigger-word lookup against the command list.

use super::*;

/// Look up the first command in `commands` whose `trigger` exactly
/// matches `typed_word` and whose `apps` filter (if set) accepts
/// `focused_basename`. Returns `None` if no command matches.
///
/// The lookup is linear because the typical user has ≤ 20
/// commands; a hash map would add complexity without measurable
/// benefit and would lose the deterministic "first match wins"
/// rule when two commands share a trigger (a config error in
/// practice — but we resolve it predictably rather than crashing).
///
/// Match is case-sensitive on the trigger by design. Most snippet
/// expanders end up wanting case-sensitive matching once users
/// have triggers like `Anrl` (capitalised expansion) vs `anrl`
/// (lowercase one).
pub fn find_matching_command<'a>(
    commands: &'a [UserCommand],
    typed_word: &str,
    focused_basename: Option<&str>,
    history: &WordHistory,
) -> Option<&'a UserCommand> {
    commands.iter().find(|c| {
        phrase_matches(c, history, typed_word)
            && (c.apps.is_empty()
                || focused_basename
                    .is_some_and(|b| c.apps.iter().any(|a| a.eq_ignore_ascii_case(b))))
    })
}
