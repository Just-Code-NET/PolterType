//! Moving a word out of an Arabic or Hebrew layout on dictionary evidence.

use poltertype_types::{DetectionVerdict, logsafe};

use super::DictionaryDetector;
use crate::consts::{
    ABJAD_ESCAPE_ADVANTAGE, ABJAD_ESCAPE_MIN_LETTERS, ARABIC_CLITICS, HEBREW_CLITICS,
};
use crate::enums::{Script, Verdict};
use crate::text::{letters_only_lower, non_word_char_count};
use crate::types::DetectionContext;

impl DictionaryDetector {
    /// A word typed on an Arabic or Hebrew keyboard that neither dictionary
    /// holds, where another layout recognises much more of it: `microsoft`,
    /// which the en-US list lacks but whose stem it knows. Shape cannot
    /// decide this, since a real word in an abjad reads as one consonant
    /// cluster, so plausibility refuses to and the choice is made here by
    /// how far each dictionary follows the word. Measured on Wikipedia text
    /// in DECISIONS.md (2026-11-04).
    pub(super) fn escape_from_abjad(
        &self,
        current: &str,
        ctx: &DetectionContext<'_>,
    ) -> Option<Verdict> {
        let letters = current.chars().count();
        if letters < ABJAD_ESCAPE_MIN_LETTERS {
            return None;
        }
        let script = Script::of(current.chars().next()?);
        let clitics = match script {
            Script::Arabic => ARABIC_CLITICS,
            Script::Hebrew => HEBREW_CLITICS,
            _ => return None,
        };
        if current.chars().any(|c| Script::of(c) != script) {
            return None;
        }
        let dicts = self.dicts.read();
        let own = dicts.get(ctx.current_layout)?;
        let share = |n: usize| n as f32 / letters as f32;
        let own_share = clitics
            .iter()
            .filter_map(|c| {
                let stem = current.strip_prefix(c)?;
                Some(c.chars().count() + own.prefix_letters(stem))
            })
            .chain([own.prefix_letters(current)])
            .map(share)
            .fold(0.0, f32::max);
        let (target, target_share) = ctx
            .candidates
            .iter()
            // A capital is a Shift, which on these keyboards types a hamza
            // form or a mark: `آخرهم` renders `Novil`. Nothing wrote it.
            .filter(|(l, t)| {
                l != ctx.current_layout
                    && non_word_char_count(t) == 0
                    && !t.chars().any(char::is_uppercase)
            })
            .filter_map(|(l, t)| {
                let word = letters_only_lower(t);
                (word.chars().count() == letters)
                    .then(|| Some((l, share(dicts.get(l)?.prefix_letters(&word)))))
                    .flatten()
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))?;
        if target_share - own_share < ABJAD_ESCAPE_ADVANTAGE {
            return None;
        }
        Some(Verdict::Switch(DetectionVerdict {
            best_layout: target.clone(),
            confidence: 0.7,
            reason: format!(
                "{target}'s dictionary follows {} further ({target_share:.2}) than {}'s ({own_share:.2})",
                logsafe::redact_word(current),
                ctx.current_layout,
            ),
        }))
    }
}
