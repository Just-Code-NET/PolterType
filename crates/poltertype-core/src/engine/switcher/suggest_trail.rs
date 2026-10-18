//! The offered word's trail: what was typed right of it while the
//! tooltip was up, kept so an accept can erase the word *and everything
//! after it* and type that back.
//!
//! The trail mirrors every keystroke the word buffer takes, and is only
//! trusted while the buffer agrees with its last segment. Anything the
//! trail cannot follow — a submission key, a backspace into the word
//! itself, a held Backspace, an overlong trail — dismisses the offer
//! rather than guess where the caret is.

use poltertype_input::{KeyDirection, KeyEvent, ReplayKey};
use poltertype_layout::LayoutId;
use poltertype_types::WordKey;

use crate::engine::buffer::{KeyKind, WordBuffer};
use crate::engine::consts::MAX_SUGGESTION_TRAIL;
use crate::engine::heuristics::is_submission_scancode;
use crate::engine::types::{PendingSuggestion, TrailKey, TrailReplay};
use crate::layouts::LayoutMapping;

use super::engine::SwitcherEngine;

const SC_SPACE: u32 = 0x39;

impl SwitcherEngine {
    /// Mirror one keystroke the buffer is about to take into the open
    /// offer's trail, or dismiss the offer when the trail cannot follow.
    pub(super) fn track_suggestion_trail(&self, ev: &KeyEvent, kind: KeyKind) {
        if ev.direction != KeyDirection::Press {
            return;
        }
        let lost = {
            let mut slot = self.pending_suggestion.lock();
            let Some(p) = slot.as_mut() else { return };
            match kind {
                // The trail already holds the word's own separator, so
                // this Enter or Tab comes later: the line may be sent,
                // and retyping it would send it again.
                KeyKind::Boundary if is_submission_scancode(ev.scancode) => true,
                KeyKind::Word | KeyKind::Boundary => {
                    p.trail.push(TrailKey {
                        key: WordKey {
                            scancode: ev.scancode,
                            shift: ev.modifiers.shift,
                            caps: ev.modifiers.caps,
                            timestamp_ms: ev.timestamp_ms,
                        },
                        word: kind == KeyKind::Word,
                    });
                    p.trail.len() > MAX_SUGGESTION_TRAIL
                }
                // Once the word's separator is gone the word is being
                // edited again, and the buffer has re-opened it.
                KeyKind::Backspace if !ev.kernel_repeat => {
                    p.trail.pop();
                    !p.trail.iter().any(|t| !t.word)
                }
                KeyKind::Backspace => true,
                KeyKind::Discard | KeyKind::EndAndDiscard => false,
            }
        };
        if lost {
            self.dismiss_suggestions(None);
        }
    }

    /// Does the buffer's screen model end exactly the way the trail
    /// does? The buffer only knows the last word, so that is what is
    /// compared; a buffer that forgot it (an overlong separator run)
    /// disagrees, which declines the accept.
    pub(super) fn trail_agrees_with_buffer(p: &PendingSuggestion, buffer: &WordBuffer) -> bool {
        let sc = |k: &WordKey| (k.scancode, k.shift);
        let mut completed: Vec<(u32, bool)> = Vec::new();
        let mut run: Vec<(u32, bool)> = Vec::new();
        let mut keys: Vec<(u32, bool)> = p.keys.iter().map(sc).collect();
        for t in &p.trail {
            if t.word {
                keys.push(sc(&t.key));
            } else {
                if !keys.is_empty() {
                    completed = std::mem::take(&mut keys);
                    run.clear();
                }
                run.push(sc(&t.key));
            }
        }
        buffer.completed().iter().map(sc).eq(completed)
            && buffer.boundary_run() == run.as_slice()
            && buffer.keys().iter().map(sc).eq(keys)
    }

    /// Type the trail back after the replacement word.
    ///
    /// A same-layout replacement retypes it key for key. A cross-layout
    /// one switches the layout under it, so it is checked again word by
    /// word: a finished word whose other-layout reading is a dictionary
    /// word and whose typed reading is not follows the switch, every
    /// other finished word and every separator keeps the characters it
    /// had. The keys glued to the word and the unfinished word at the
    /// caret follow the switch as before — the first belong to the
    /// word, the second is what the user goes on typing.
    pub(super) fn replay_trail(
        &self,
        trail: &[TrailKey],
        from: &LayoutId,
        to: &LayoutId,
    ) -> Option<TrailReplay> {
        let orig = self.layouts.get(from)?;
        let target = self.layouts.get(to)?;
        let cross = from != to;
        let caps_on = self.caps_on();
        let mut out = TrailReplay {
            keys: Some(Vec::with_capacity(trail.len())),
            text: String::new(),
            stash_stale: false,
        };
        let mut i = 0;
        while i < trail.len() {
            if !trail[i].word {
                let k = trail[i].key;
                if cross && !is_submission_scancode(k.scancode) && k.scancode != SC_SPACE {
                    out.preserve(k, orig, target, caps_on);
                } else {
                    out.separator(k, target);
                }
                i += 1;
                continue;
            }
            let end = i + trail[i..].iter().take_while(|t| t.word).count();
            let keys: Vec<WordKey> = trail[i..end].iter().map(|t| t.key).collect();
            let finished = i > 0 && end < trail.len();
            if !cross || !finished || self.trail_word_follows(&keys, from, to) {
                out.verbatim(&keys, target);
            } else {
                for k in keys {
                    out.preserve(k, orig, target, caps_on);
                }
            }
            i = end;
        }
        out.stash_stale |= out.keys.is_none();
        Some(out)
    }

    fn trail_word_follows(&self, keys: &[WordKey], from: &LayoutId, to: &LayoutId) -> bool {
        let (Some(provider), Some(orig), Some(target)) = (
            self.suggester.as_ref(),
            self.layouts.get(from),
            self.layouts.get(to),
        ) else {
            return false;
        };
        provider.is_known(to, &target.translate_buffer(keys))
            && !provider.is_known(from, &orig.translate_buffer(keys))
    }
}

impl TrailReplay {
    fn push(&mut self, key: Option<ReplayKey>, ch: char) {
        self.text.push(ch);
        match key {
            Some(k) => {
                if let Some(keys) = self.keys.as_mut() {
                    keys.push(k);
                }
            }
            None => self.keys = None,
        }
    }

    fn verbatim(&mut self, keys: &[WordKey], target: &LayoutMapping) {
        self.text.push_str(&target.translate_buffer(keys));
        if let Some(out) = self.keys.as_mut() {
            out.extend(keys.iter().map(|k| ReplayKey {
                scancode: k.scancode,
                shift: k.shift,
            }));
        }
    }

    /// Enter and Tab are retyped as Space: re-pressing them submits the
    /// line or moves focus.
    fn separator(&mut self, k: WordKey, target: &LayoutMapping) {
        let scancode = if is_submission_scancode(k.scancode) {
            SC_SPACE
        } else {
            k.scancode
        };
        let shift = k.shift && scancode == k.scancode;
        let ch = target
            .translate_key(WordKey {
                scancode,
                shift,
                caps: false,
                timestamp_ms: 0,
            })
            .unwrap_or(' ');
        self.push(Some(ReplayKey { scancode, shift }), ch);
    }

    /// The character `k` typed under `orig`, on whichever key types it
    /// under `target`.
    fn preserve(
        &mut self,
        k: WordKey,
        orig: &LayoutMapping,
        target: &LayoutMapping,
        caps_on: bool,
    ) {
        let Some(ch) = orig.translate_key(k) else {
            self.verbatim(&[k], target);
            return;
        };
        let key = if target.translate_key(k) == Some(ch) {
            Some(ReplayKey {
                scancode: k.scancode,
                shift: k.shift,
            })
        } else {
            self.stash_stale = true;
            target
                .press_for_char(ch, caps_on)
                .map(|(scancode, shift)| ReplayKey { scancode, shift })
        };
        self.push(key, ch);
    }
}
