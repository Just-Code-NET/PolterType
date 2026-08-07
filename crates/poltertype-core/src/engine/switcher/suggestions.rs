//! Spelling-suggestion offers: when to offer, how an accept is
//! validated, and applying the chosen replacement through the same
//! absorb → delete → replay machinery as layout corrections.
//!
//! Lifecycle of one offer:
//!
//! 1. `decide()` kept the word, the word is not in the current
//!    language's dictionary, no suppression applies →
//!    [`SwitcherEngine::maybe_offer_suggestions`] stamps a generation,
//!    stashes a [`PendingSuggestion`] and emits
//!    [`SwitcherEvent::SuggestionsReady`] for the tooltip.
//! 2. The user clicks an entry (app sends
//!    [`EngineCommand::AcceptSuggestion`]) or presses the accept
//!    chord + digit (matched off the key stream in `commands.rs`).
//! 3. [`SwitcherEngine::accept_suggestion`] re-validates: right
//!    generation, deadline not passed, and the mistyped word is
//!    still the last completed word the buffer can vouch for.
//! 4. The replacement is emitted via `apply_correction` — same-layout
//!    for spelling entries, with a real layout switch for the
//!    below-threshold cross-layout entry.
//!
//! Anything that invalidates the screen position of the word (next
//! word committed, caret moved, pause, settings reload, tooltip
//! timeout) dismisses the offer via
//! [`SwitcherEngine::dismiss_suggestions`].

use std::sync::atomic::Ordering;
use std::time::Instant;

use crossbeam_channel::Receiver;
use poltertype_detect::letters_only_lower;
use poltertype_input::{KeyEvent, ReplayKey};
use poltertype_layout::LayoutId;
use poltertype_types::WordKey;
use tracing::{debug, warn};

use crate::engine::buffer::WordBuffer;
use crate::engine::enums::{DictionaryAddOrigin, SwitcherEvent};
use crate::engine::heuristics::is_submission_scancode;
use crate::engine::types::{
    AcceptModifiers, Correction, FrozenScreen, PendingSuggestion, PlannedReplacement,
    SuggestionAction, SuggestionEntry,
};
use crate::settings::Settings;

use super::engine::SwitcherEngine;

/// How long a click-frozen offer stays acceptable. Long enough for
/// the tooltip's `Accepted` event to cross popup thread → app loop →
/// engine command channel; short enough that a click *elsewhere*
/// (which also freezes, because the engine can't tell the difference)
/// can't authorise a replacement after the user has moved on.
const CLICK_GRACE: std::time::Duration = std::time::Duration::from_millis(500);

impl SwitcherEngine {
    /// Offer suggestions for a just-completed word the engine decided
    /// to keep. Quiet unless every gate passes: feature enabled, a
    /// provider wired, token long enough, not whitelisted, and not a
    /// known word of the current language.
    pub(super) fn maybe_offer_suggestions(
        &self,
        keys: &[WordKey],
        current_text: &str,
        current_layout: &LayoutId,
        low_conf_alt: Option<(LayoutId, String)>,
        snap: &Settings,
    ) {
        let Some(provider) = self.suggester.as_ref() else {
            return;
        };
        if !snap.suggestions.enabled {
            return;
        }
        let stripped = letters_only_lower(current_text);
        if stripped.chars().count() < 3 {
            return;
        }
        // The whitelist means "never touch this word" — that includes
        // not nagging about it.
        if snap.exceptions.is_whitelisted(&stripped) {
            return;
        }
        if provider.is_known(current_layout, current_text) {
            return;
        }

        let max = snap.suggestions.max_clamped();
        let mut entries: Vec<SuggestionEntry> = Vec::with_capacity(max);
        // The below-threshold cross-layout candidate leads the list:
        // when it exists it is a *dictionary word* of another active
        // language, which is a stronger signal than any same-layout
        // fuzzy match.
        if let Some((alt_layout, alt_text)) = low_conf_alt {
            entries.push(SuggestionEntry {
                text: alt_text,
                switch_to: Some(alt_layout),
                action: SuggestionAction::Replace,
            });
        }
        for s in provider.suggest(current_layout, current_text, max) {
            if entries.len() >= max {
                break;
            }
            if entries.iter().any(|e| e.text == s.text) {
                continue;
            }
            entries.push(SuggestionEntry {
                text: s.text,
                switch_to: None,
                action: SuggestionAction::Replace,
            });
        }
        if entries.is_empty() {
            // Unknown word with no nearby dictionary entries either —
            // likely cross-layout gibberish or jargon. Stay quiet.
            // (Length only — the token itself never reaches the log.)
            debug!(
                token_len = stripped.chars().count(),
                "no suggestion candidates — staying quiet"
            );
            return;
        }
        // Last row: "add to dictionary" — the escape hatch for
        // jargon, names and project vocabulary the tooltip would
        // otherwise keep flagging. Rides along only when a tooltip
        // shows anyway (a tooltip whose ONLY content is "add to
        // dictionary" would itself be the noise it exists to stop).
        // Trimmed to keep the total digit-addressable (1..=9).
        entries.truncate(8);
        entries.push(SuggestionEntry {
            text: current_text.to_owned(),
            switch_to: None,
            action: SuggestionAction::AddToDictionary,
        });

        let generation = self.suggestion_generation.fetch_add(1, Ordering::Relaxed) + 1;
        let accept = AcceptModifiers::parse(&snap.suggestions.accept_modifiers);
        let timeout = snap.suggestions.timeout();
        *self.pending_suggestion.lock() = Some(PendingSuggestion {
            generation,
            keys: keys.to_vec(),
            rendered: current_text.to_owned(),
            layout: current_layout.clone(),
            entries: entries.clone(),
            deadline: Instant::now() + timeout,
            accept,
            frozen: None,
        });
        debug!(
            generation,
            candidates = entries.len(),
            "suggestion offer stashed" // never the text itself
        );
        let _ = self.out_tx.send(SwitcherEvent::SuggestionsReady {
            generation,
            original: current_text.to_owned(),
            entries,
            timeout,
            accept_modifiers: if accept.is_some() {
                snap.suggestions.accept_modifiers.clone()
            } else {
                String::new()
            },
        });
    }

    /// Is a suggestion offer currently pending and within its
    /// deadline? Consulted by the run loop's idle-hygiene path — a
    /// live tooltip suspends the completed-word stash's idle expiry.
    pub(super) fn has_live_suggestion(&self) -> bool {
        self.pending_suggestion
            .lock()
            .as_ref()
            .is_some_and(|p| Instant::now() <= p.deadline)
    }

    /// A pointer press is about to abandon the buffer — freeze the
    /// screen model into the pending offer first, in case this click
    /// lands on the tooltip (whose `Accepted` event arrives through
    /// the command channel a beat later). Only freezes while the
    /// buffer still vouches for the offered word.
    pub(super) fn freeze_suggestion_for_click(&self, buffer: &WordBuffer) {
        let mut slot = self.pending_suggestion.lock();
        let Some(p) = slot.as_mut() else { return };
        let now = Instant::now();
        if now > p.deadline {
            return;
        }
        let same_word = buffer.completed().len() == p.keys.len()
            && buffer
                .completed()
                .iter()
                .zip(&p.keys)
                .all(|(a, b)| a.scancode == b.scancode && a.shift == b.shift);
        if !same_word {
            return;
        }
        p.frozen = Some(FrozenScreen {
            run: buffer.boundary_run().to_vec(),
            tail: buffer.keys().to_vec(),
            until: now + CLICK_GRACE,
        });
    }

    /// True while a click-grace window is open — the run loop skips
    /// the pointer-abandon dismissal so a tooltip click can still be
    /// honoured.
    pub(super) fn has_click_grace(&self) -> bool {
        self.pending_suggestion
            .lock()
            .as_ref()
            .and_then(|p| p.frozen.as_ref())
            .is_some_and(|f| Instant::now() <= f.until)
    }

    /// Per-event grace bookkeeping: a frozen offer dies on the first
    /// non-pointer keypress (the user clicked elsewhere and moved on
    /// — the caret is somewhere we can't vouch for) or once the grace
    /// window lapses.
    pub(super) fn click_grace_tick(&self, ev: &KeyEvent) {
        let stale = {
            let slot = self.pending_suggestion.lock();
            match slot.as_ref().and_then(|p| p.frozen.as_ref()) {
                Some(f) => {
                    Instant::now() > f.until
                        || (!ev.injected
                            && ev.direction == poltertype_input::KeyDirection::Press
                            && ev.scancode != poltertype_types::SC_POINTER_BUTTON)
                }
                None => false,
            }
        };
        if stale {
            self.dismiss_suggestions(None);
        }
    }

    /// Drop the in-flight offer, if any, and tell the tooltip to
    /// hide. `only_generation` restricts the dismissal to one
    /// specific offer (popup-side timeouts race new offers).
    pub(super) fn dismiss_suggestions(&self, only_generation: Option<u64>) {
        let generation = {
            let mut slot = self.pending_suggestion.lock();
            match slot.as_ref() {
                Some(p) if only_generation.is_none_or(|g| g == p.generation) => {
                    let g = p.generation;
                    *slot = None;
                    g
                }
                _ => return,
            }
        };
        let _ = self
            .out_tx
            .send(SwitcherEvent::SuggestionsDismissed { generation });
    }

    /// Handle an accept (tooltip click or digit chord). Validates the
    /// generation, the deadline and — critically — that the mistyped
    /// word is still the last completed word the buffer can vouch
    /// for; anything else is silently declined (the tooltip is
    /// already gone or lying about the screen).
    pub(super) fn accept_suggestion(
        &self,
        generation: u64,
        index: usize,
        typed_digit: bool,
        from_pointer: bool,
        buffer: &mut WordBuffer,
        key_rx: &Receiver<KeyEvent>,
    ) {
        // Atomic take — same duplicate-fire discipline as the manual
        // switch-last hotkey: a second fire from auto-repeat or a
        // double-click finds `None` and exits.
        let taken = {
            let mut slot = self.pending_suggestion.lock();
            if slot.as_ref().is_some_and(|p| p.generation == generation) {
                slot.take()
            } else {
                None
            }
        };
        let Some(pending) = taken else {
            debug!(generation, "suggestion accept ignored: stale generation");
            return;
        };
        // The offer is consumed whatever happens next — make sure the
        // tooltip agrees (idempotent for the click path, where the
        // popup hid itself optimistically).
        let _ = self
            .out_tx
            .send(SwitcherEvent::SuggestionsDismissed { generation });

        if *self.paused.read() {
            return;
        }
        if Instant::now() > pending.deadline {
            debug!(generation, "suggestion accept ignored: offer expired");
            return;
        }
        let Some(entry) = pending.entries.get(index).cloned() else {
            debug!(generation, index, "suggestion accept ignored: bad index");
            return;
        };

        // "Add to dictionary" touches no text — no screen validation
        // needed (it stays meaningful even after the user typed on).
        // The app owns the overlay file and the dictionary reload.
        if entry.action == SuggestionAction::AddToDictionary {
            let _ = self.out_tx.send(SwitcherEvent::AddToDictionary {
                layout: pending.layout.clone(),
                word: entry.text,
                origin: DictionaryAddOrigin::Tooltip,
            });
            return;
        }

        // Two ways the screen state can be vouched for:
        //
        // * The live buffer still holds the offered word (chord path,
        //   or a click whose `Accepted` event outran its own key-
        //   stream observation) → read separators/tail from it.
        // * The buffer was just abandoned by the click's pointer
        //   press, but the state was frozen at that instant and the
        //   grace window is open → use the frozen copy (a click ON
        //   the overlay never reached the app, so the screen is
        //   exactly as frozen).
        let same_word = buffer.completed().len() == pending.keys.len()
            && buffer
                .completed()
                .iter()
                .zip(&pending.keys)
                .all(|(a, b)| a.scancode == b.scancode && a.shift == b.shift);
        let screen = if same_word {
            Some((buffer.boundary_run().to_vec(), buffer.keys().to_vec()))
        } else {
            match pending.frozen.as_ref() {
                Some(f) if Instant::now() <= f.until => Some((f.run.clone(), f.tail.clone())),
                _ => None,
            }
        };
        let Some((run, tail)) = screen else {
            debug!(
                generation,
                "suggestion accept declined: word no longer last on screen"
            );
            return;
        };
        // A click-sourced accept has exactly one physical click in
        // flight; the absorb machinery must swallow it rather than
        // abort. (When the pointer press was already consumed —
        // frozen path — an unused allowance is harmless: it only
        // ever ignores pointer presses, which always mean "caret
        // moved" for every OTHER purpose.)
        let click_allowance = usize::from(from_pointer);
        let Some(plan) =
            self.plan_suggestion_replacement(&pending, &entry, &run, &tail, typed_digit)
        else {
            return;
        };
        self.apply_suggestion_replacement(&pending, &entry, &plan, click_allowance, buffer, key_rx);
    }

    /// Work out what the replacement is: which layout to end up in,
    /// how much of the screen to delete, what to replay in its place,
    /// and how it reads once typed.
    ///
    /// `None` declines the accept, leaving the user's text untouched —
    /// every reason to give up lives here rather than half-way through
    /// emitting.
    fn plan_suggestion_replacement(
        &self,
        pending: &PendingSuggestion,
        entry: &SuggestionEntry,
        boundary_run: &[(u32, bool)],
        tail_keys: &[WordKey],
        typed_digit: bool,
    ) -> Option<PlannedReplacement> {
        let target_layout = entry
            .switch_to
            .clone()
            .unwrap_or_else(|| pending.layout.clone());
        let Some(target_mapping) = self.layouts.get(&target_layout) else {
            warn!(%target_layout, "suggestion target layout not in DB");
            return None;
        };

        // Screen model left of the caret:
        // `<word><boundary_run><in-progress keys>[<chord digit>][caret]`
        // — delete all of it, retype with the word replaced.
        let backspaces =
            pending.keys.len() + boundary_run.len() + tail_keys.len() + usize::from(typed_digit);
        if boundary_run.is_empty() {
            // The separator the offer was made over is gone (it can
            // only shrink via backspacing, which re-opens the word and
            // clears `completed()` — but belt and braces).
            debug!("suggestion accept declined: boundary run empty");
            return None;
        }

        // The word itself: cross-layout entries replay the original
        // scancodes under the switched layout (exactly what
        // force_switch_last does); spelling entries reverse-map the
        // suggestion text to scancodes. A character the current layout
        // can't type (uk apostrophe) falls back to text injection.
        let word_replay: Option<Vec<ReplayKey>> = if entry.switch_to.is_some() {
            Some(
                pending
                    .keys
                    .iter()
                    .map(|k| ReplayKey {
                        scancode: k.scancode,
                        shift: k.shift,
                    })
                    .collect(),
            )
        } else {
            entry
                .text
                .chars()
                .map(|c| {
                    target_mapping
                        .key_for_char(c)
                        .map(|(scancode, shift)| ReplayKey { scancode, shift })
                })
                .collect()
        };

        // Separators + the user's in-progress next word, re-emitted
        // after the replacement. Enter/Tab in a separator run must
        // not be re-pressed (submits the line) — substitute Space,
        // same as the manual force-switch path.
        let extra: Vec<ReplayKey> = boundary_run
            .iter()
            .map(|&(sc, shift)| {
                if is_submission_scancode(sc) {
                    ReplayKey {
                        scancode: 0x39,
                        shift: false,
                    }
                } else {
                    ReplayKey {
                        scancode: sc,
                        shift,
                    }
                }
            })
            .chain(tail_keys.iter().map(|k| ReplayKey {
                scancode: k.scancode,
                shift: k.shift,
            }))
            .collect();

        // Rendered form of the full replacement — the `Corrected`
        // event payload and the text-injection fallback body.
        let mut corrected = entry.text.clone();
        for rk in &extra {
            let ch = target_mapping
                .translate_key(WordKey {
                    scancode: rk.scancode,
                    shift: rk.shift,
                    timestamp_ms: 0,
                })
                .or(match rk.scancode {
                    0x39 => Some(' '),
                    _ => None,
                })
                .unwrap_or(' ');
            corrected.push(ch);
        }

        let full_replay: Option<Vec<ReplayKey>> = word_replay.map(|mut w| {
            w.extend(extra.iter().copied());
            w
        });
        let reason = if entry.switch_to.is_some() {
            "cross-layout suggestion accepted"
        } else {
            "spelling suggestion accepted"
        };

        // The replacement word in scancodes, for re-pointing the
        // buffer's stash afterwards. Worked out here because this is
        // where the target mapping is in hand; `None` when the layout
        // cannot type every character, which is exactly the case where
        // the stash must be dropped rather than re-pointed.
        let replacement_keys: Option<Vec<WordKey>> = {
            let keys: Vec<WordKey> = entry
                .text
                .chars()
                .filter_map(|c| target_mapping.key_for_char(c))
                .map(|(scancode, shift)| WordKey {
                    scancode,
                    shift,
                    timestamp_ms: 0,
                })
                .collect();
            (keys.len() == entry.text.chars().count()).then_some(keys)
        };

        Some(PlannedReplacement {
            target_layout,
            backspaces,
            corrected,
            replay: full_replay,
            reason,
            replacement_keys,
        })
    }

    /// Emit the replacement. Reuses `apply_correction` wholesale: it
    /// already owns the absorb window, echo bookkeeping, compensation
    /// loop and buffer re-seeding, and every one of those hazards
    /// applies here identically.
    fn apply_suggestion_replacement(
        &self,
        pending: &PendingSuggestion,
        entry: &SuggestionEntry,
        plan: &PlannedReplacement,
        click_allowance: usize,
        buffer: &mut WordBuffer,
        key_rx: &Receiver<KeyEvent>,
    ) {
        let snap = self.settings.snapshot();
        let applied = self.apply_correction(
            &Correction {
                from: &pending.layout,
                to: &plan.target_layout,
                original: &pending.rendered,
                corrected: &plan.corrected,
                backspaces: plan.backspaces,
                reason: plan.reason,
                play_sound: snap.general.sound_on_correct,
                replay_keys: plan.replay.as_deref(),
                pointer_click_allowance: click_allowance,
            },
            Some((key_rx, buffer)),
        );
        if !applied {
            return;
        }

        let _ = self.out_tx.send(SwitcherEvent::SuggestionApplied {
            original: pending.rendered.clone(),
            replacement: entry.text.clone(),
        });

        // Keep the stashes coherent with the new screen contents.
        //
        // * Cross-layout entry: the scancodes on screen are unchanged
        //   (same keys, new layout) — the buffer stash stays valid.
        // * Spelling entry: the word now has *different* scancodes.
        //   Re-point the buffer's completed-word stash at them (or
        //   forget it when text injection was used and no scancode
        //   form exists) so backspacing across the boundary re-opens
        //   the right thing.
        //
        // The manual switch-last stash is dropped in both cases:
        // re-transliterating a word the user just hand-picked is
        // never what the hotkey should do next.
        if entry.switch_to.is_none() {
            let still_same = buffer.completed().len() == pending.keys.len()
                && buffer
                    .completed()
                    .iter()
                    .zip(&pending.keys)
                    .all(|(a, b)| a.scancode == b.scancode && a.shift == b.shift);
            if still_same {
                // `None` means text injection was used and no scancode
                // form exists — forget the stash rather than point it
                // at something that is not on screen.
                buffer.replace_completed(plan.replacement_keys.clone().unwrap_or_default());
            }
        }
        *self.last_word.write() = None;
    }
}
