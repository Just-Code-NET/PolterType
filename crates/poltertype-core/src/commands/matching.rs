//! Trigger-word lookup against the command list.

use super::*;

/// The first command whose `trigger` exactly matches `typed_word` and
/// whose `apps` filter accepts `focused_basename`, or `None`.
///
/// Linear because the typical user has ≤ 20 commands; a hash map would
/// add complexity for no measurable gain and lose the deterministic
/// first-match-wins rule when two commands share a trigger.
///
/// Case-sensitive on the trigger by design — snippet expanders end up
/// wanting it once users have `Anrl` alongside `anrl`.
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
