//! Per-completed-word decision: candidate filtering, smart-command
//! lookup, the pre-decision filters, and the detector pipeline.

use crossbeam_channel::Receiver;
use poltertype_detect::{Verdict, letters_only_lower, looks_like_code_token};
use poltertype_input::{KeyEvent, ReplayKey};
use poltertype_layout::LayoutId;
use poltertype_types::{SwitchAction, logsafe};
use tracing::{debug, warn};

use crate::commands::{erase_len, find_matching_command};
use crate::engine::buffer::WordBuffer;
use crate::engine::enums::SwitcherEvent;
use crate::engine::heuristics::{
    app_is_disabled, boundary_key_for, is_layout_eligible, is_structural_boundary,
    is_submission_boundary, looks_like_all_caps, render_for_code_check,
};
use crate::engine::types::{Correction, LastWord, WordBoundaryKey};

use super::engine::SwitcherEngine;

impl SwitcherEngine {
    /// Best-effort current-layout translate. `None` when the OS
    /// cannot be queried or the scancode is not in the mapping table —
    /// both normal for control / OEM keys.
    pub(super) fn translate_via_current_layout(
        &self,
        scancode: u32,
        shift: bool,
        caps: bool,
    ) -> Option<char> {
        let current = self.layout_switcher.current().ok()?;
        let mapping = self.layouts.get(&current)?;
        mapping.translate_key(poltertype_types::WordKey {
            scancode,
            shift,
            caps,
            timestamp_ms: 0,
        })
    }

    /// The word the user is still typing, shaped as something the
    /// manual switch-last hotkey can act on.
    ///
    /// The stash proper is only written when a word is *closed*, so
    /// until now the hotkey did nothing at all for a word with no
    /// separator after it yet — it logged "no last word stashed" and
    /// returned. That is the gesture people arrive with from Punto
    /// Switcher and Caramba: type, see the wrong layout, press the key,
    /// before any space is involved. Measured on KDE Plasma Wayland
    /// against 0.19.0 (issues #34, #32).
    ///
    /// `None` while an unrecorded remainder sits at the caret: these
    /// keys then continue a word already on screen, and converting
    /// only them would split it across two layouts. A taint left by an
    /// interruption the caret has since *moved away from* is not that
    /// — see
    /// [`WordBuffer::remainder_at_caret`](crate::engine::buffer::WordBuffer::remainder_at_caret).
    pub(super) fn word_in_progress(&self, buffer: &WordBuffer) -> Option<LastWord> {
        if buffer.remainder_at_caret() {
            // Its own line: lumped in with the empty-buffer case, a
            // decline here reads in a log exactly like a hotkey that
            // never fired (issue #44).
            debug!("manual switch-last declined: the word under the caret was only half-observed");
            return None;
        }
        let keys = buffer.keys().to_vec();
        if keys.is_empty() {
            return None;
        }
        let layout = self
            .word_layout
            .read()
            .clone()
            .or_else(|| self.layout_switcher.current().ok())?;
        let rendered = self.layouts.get(&layout)?.translate_buffer(&keys);
        Some(LastWord {
            keys,
            rendered,
            layout,
            boundary: None,
            // An unfinished word has never been through `decide`, so
            // there is no correction of ours to undo.
            corrected_to: None,
            user_placed: false,
        })
    }

    pub(super) fn decide(
        &self,
        buffer: &mut WordBuffer,
        boundary_scancode: u32,
        boundary_shift: bool,
        started_clean: bool,
        key_rx: &Receiver<KeyEvent>,
    ) {
        let snap = self.settings.snapshot();
        let keys = buffer.completed().to_vec();
        if keys.is_empty() {
            return;
        }
        // No global min_word_length gate: each detector decides for
        // itself.

        let current_layout = match self.layout_switcher.current() {
            Ok(l) => l,
            Err(e) => {
                warn!(?e, "could not query current layout; skipping decision");
                return;
            }
        };

        // The OS-active layer matters most: an unreachable layout
        // reaching the detector means `switch_to` rejects it *after* the
        // backspaces went out, destroying the word. A failed query fails
        // open; `apply_correction` pre-flights again.
        let os_active: Option<Vec<LayoutId>> = match self.layout_switcher.list_active() {
            Ok(list) => Some(list),
            Err(e) => {
                warn!(
                    ?e,
                    "could not list active OS layouts; skipping OS-active filter"
                );
                None
            }
        };
        let active: &[LayoutId] = &snap.languages.active;
        let ignored: &[LayoutId] = &snap.languages.ignored;
        let candidates: Vec<(LayoutId, String)> = self
            .layouts
            .iter()
            .filter(|(id, _)| {
                is_layout_eligible(id, &current_layout, active, ignored, os_active.as_deref())
            })
            .map(|(id, m)| (id.clone(), m.translate_buffer(&keys)))
            .collect();

        // Not necessarily the layout active now: the user may have
        // switched by hand between the last letter and the key that
        // closed the word. See `word_layout`.
        let typed_layout = self
            .word_layout
            .read()
            .clone()
            .unwrap_or_else(|| current_layout.clone());

        // Rendered under the layout it was typed in, because that is
        // what is on screen.
        let current_text = self
            .layouts
            .get(&typed_layout)
            .map(|m| m.translate_buffer(&keys))
            .unwrap_or_default();

        // Only ever called for separators, which no lock state can
        // change the level of.
        let render_key = |scancode: u32, shift: bool| {
            self.layouts.get(&current_layout).and_then(|m| {
                m.translate_key(poltertype_types::WordKey {
                    scancode,
                    shift,
                    caps: false,
                    timestamp_ms: 0,
                })
            })
        };

        // The separator the word opened after — `/` of `/tmp`, `@` of
        // `@nick`. Read before anything below can mutate the buffer.
        let lead_char = buffer
            .completed_lead()
            .and_then(|(sc, shift)| render_key(sc, shift));

        let boundary_char = render_key(boundary_scancode, boundary_shift)
            .or(match boundary_scancode {
                0x39 => Some(' '),
                0x1C | 0x60 => Some('\n'), // Enter / numpad Enter
                0x0F => Some('\t'),
                _ => None,
            })
            .unwrap_or(' ');

        // Stashed before any filter below can return: the manual
        // switch-last hotkey works on words the automatic path skips.
        *self.last_word.write() = Some(LastWord {
            keys: keys.clone(),
            rendered: current_text.clone(),
            layout: typed_layout.clone(),
            boundary: Some(WordBoundaryKey {
                ch: boundary_char,
                scancode: boundary_scancode,
                shift: boundary_shift,
            }),
            // Filled in by `apply_correction`; the stash is written
            // before the decision is made.
            corrected_to: None,
            user_placed: false,
        });

        // Paused stops the engine deciding, not the engine watching:
        // the stash above is what the manual hotkey acts on, and a
        // person who turned auto-switch off is exactly the person
        // reaching for it (issue #36).
        if *self.paused.read() {
            debug!("paused — word stashed for the manual hotkey, no automatic decision");
            return;
        }

        // A hand switch between the word and the key that closed it.
        // The word on screen is still the *old* layout's rendering, so
        // reading it under the new one turns correct text into
        // gibberish, and "correcting" that retypes a word that was
        // already right. Nothing below can tell the two halves apart.
        if typed_layout != current_layout {
            debug!(
                typed = %typed_layout,
                current = %current_layout,
                "skipping auto-switch: layout changed while this word was being typed"
            );
            let _ = self.out_tx.send(SwitcherEvent::KeptCurrent {
                reason: format!(
                    "layout changed from {typed_layout} to {current_layout} while {} was being \
                     typed",
                    logsafe::redact_word(&current_text)
                ),
            });
            return;
        }

        // Before the auto-switch filters: expansion is a direct user
        // intent, not a guess. Matching is on the *current* layout's
        // rendering, so the same physical keys under another layout fall
        // through to layout-correction.
        let focused_basename = self.focus_tracker.focused_exe().and_then(|exe| {
            std::path::Path::new(&exe)
                .file_name()
                .and_then(|f| f.to_str())
                .map(str::to_owned)
        });
        let history = self.word_history.read().clone();
        if let Some(cmd) = find_matching_command(
            &snap.commands,
            &current_text,
            focused_basename.as_deref(),
            &history,
        ) {
            // Counting keys rather than rendered chars survives
            // scancodes the mapping table cannot render.
            self.dispatch_smart_command(cmd, erase_len(cmd, keys.len()), boundary_char);
            // The trigger text is gone from screen: not re-openable by
            // backspace, not matchable again.
            buffer.forget_completed();
            self.word_history.write().clear();
            return;
        }
        // Not a trigger — remember it as a possible first half.
        self.word_history
            .write()
            .push_in(focused_basename.as_deref(), &current_text);

        // Pre-decision filters, automatic decisions only. The manual
        // switch-last hotkey calls `force_switch_last` and bypasses
        // every one of them.

        // Filter 0: `word_whitelist` — the only filter that is a direct
        // statement of intent rather than a heuristic, so it goes first.
        if snap
            .exceptions
            .is_whitelisted(&letters_only_lower(&current_text))
        {
            debug!(
                token = %logsafe::redact_word(&current_text),
                "skipping auto-switch: word on the whitelist"
            );
            let _ = self.out_tx.send(SwitcherEvent::KeptCurrent {
                reason: format!(
                    "{} is on the word whitelist",
                    logsafe::redact_word(&current_text)
                ),
            });
            return;
        }

        // Filter 0a: submission / navigation boundary. Replaying Enter
        // or Tab runs a command or sends a message, and the line is
        // already gone anyway.
        if is_submission_boundary(boundary_char) {
            debug!(
                token = %logsafe::redact_word(&current_text),
                "skipping auto-switch: submission boundary (Enter/Tab)"
            );
            let _ = self.out_tx.send(SwitcherEvent::KeptCurrent {
                reason: format!(
                    "submission boundary after {} — not re-emitting Enter/Tab",
                    logsafe::redact_word(&current_text)
                ),
            });
            return;
        }

        // Filter 0b: structural boundary (`:` `/` `\` `@` `=` `#` `&`)
        // means URL / path / email / code, not prose. Switching `http`
        // to `реез` would corrupt what the user is half-way through.
        if is_structural_boundary(boundary_char) {
            debug!(
                token = %logsafe::redact_word(&current_text),
                boundary = %boundary_char,
                "skipping auto-switch: structural boundary"
            );
            let _ = self.out_tx.send(SwitcherEvent::KeptCurrent {
                reason: format!(
                    "structural boundary `{boundary_char}` after {} — likely URL / path / email / code",
                    logsafe::redact_word(&current_text)
                ),
            });
            return;
        }

        // Filter 0b-bis: the same characters *before* the word. Filter
        // 0b only ever sees what ends a token, and a path segment ends
        // with an ordinary space: `/tmp ` reached the detectors as a
        // bare `tmp` and came back as `еьз`.
        if let Some(lead) = lead_char.filter(|c| is_structural_boundary(*c)) {
            debug!(
                token = %logsafe::redact_word(&current_text),
                lead = %lead,
                "skipping auto-switch: structural prefix"
            );
            let _ = self.out_tx.send(SwitcherEvent::KeptCurrent {
                reason: format!(
                    "structural prefix `{lead}` before {} — likely URL / path / email / code",
                    logsafe::redact_word(&current_text)
                ),
            });
            return;
        }

        // Filter 0b-ter: a hyphen *opens* the token. `-` is a word
        // character — that is what keeps `well-known` whole — so a
        // command-line flag arrives as one token with no separator for
        // the two filters above to read, and the code-token guard below
        // looks for underscores, digits and camel case, none of which a
        // bare `--wsl` has. No prose word opens with a hyphen; `--wsl `
        // came back as `--цід `.
        if current_text.starts_with('-') {
            debug!(
                token = %logsafe::redact_word(&current_text),
                "skipping auto-switch: token opens with a hyphen (command-line flag)"
            );
            let _ = self.out_tx.send(SwitcherEvent::KeptCurrent {
                reason: format!(
                    "{} opens with a hyphen — likely a command-line flag",
                    logsafe::redact_word(&current_text)
                ),
            });
            return;
        }

        // Filter 0c: ALL-CAPS is deliberate spelling-out, not a wrong
        // layout — and it renders as letter-like bait for the detector.
        // Held Shift catches it everywhere; Caps Lock only on
        // Linux/Wayland, where the listener folds caps into the shift
        // bit.
        if snap.engine.suppress_for_all_caps && looks_like_all_caps(&current_text) {
            debug!(
                token = %logsafe::redact_word(&current_text),
                "skipping auto-switch: word is ALL CAPS (likely abbreviation)"
            );
            let _ = self.out_tx.send(SwitcherEvent::KeptCurrent {
                reason: format!(
                    "{} is ALL CAPS — likely an abbreviation, not a wrong-layout word",
                    logsafe::redact_word(&current_text)
                ),
            });
            return;
        }

        // Filter 1: focused app on the disabled list.
        if let Some(exe) = self.focus_tracker.focused_exe() {
            if app_is_disabled(&exe, &snap.exceptions.disabled_apps) {
                debug!(%exe, "skipping auto-switch: app on disabled_apps list");
                let _ = self.out_tx.send(SwitcherEvent::KeptCurrent {
                    reason: format!("app `{exe}` on disabled_apps list"),
                });
                return;
            }
        }

        // Filter 2: identifier-shaped token. Fed a *cleaned* rendering,
        // or a Ukrainian `ж` under en-US shows up as a mid-string `;`
        // and the heuristic calls prose "code".
        let token_for_code_check =
            render_for_code_check(&keys, &current_layout, &self.layouts, &current_text);
        if snap.engine.suppress_in_identifiers && looks_like_code_token(&token_for_code_check) {
            debug!(
                token = %logsafe::redact_word(&current_text),
                cleaned = %logsafe::redact_word(&token_for_code_check),
                "skipping auto-switch: looks like code identifier"
            );
            let _ = self.out_tx.send(SwitcherEvent::KeptCurrent {
                reason: format!(
                    "token {} looks like an identifier",
                    logsafe::redact_word(&current_text)
                ),
            });
            return;
        }

        let ctx = poltertype_detect::DetectionContext {
            current_layout: &current_layout,
            candidates: &candidates,
            recent_context: "",
        };

        // Priority order; first non-NoOpinion verdict wins, including a
        // `Keep` veto.
        let mut chosen: Option<Verdict> = None;
        for d in &self.detectors {
            match d.judge(&ctx) {
                Verdict::NoOpinion => continue,
                v => {
                    chosen = Some(v);
                    break;
                }
            }
        }

        // Below the confidence threshold: not auto-applied, but offered
        // in the suggestions tooltip for the user to decide.
        let mut low_conf_alt: Option<(LayoutId, String)> = None;

        let action = match chosen {
            Some(Verdict::Keep { reason }) => SwitchAction::KeepCurrent {
                reason: format!("veto by detector: {reason}"),
            },
            Some(Verdict::Switch(v)) if v.confidence >= snap.engine.confidence_threshold => {
                let target_text = candidates
                    .iter()
                    .find(|(l, _)| l == &v.best_layout)
                    .map(|(_, t)| t.clone())
                    .unwrap_or_default();
                // The boundary key already reached the focused app, so
                // it has to be deleted and re-emitted after the word.
                let mut corrected_with_boundary = target_text;
                corrected_with_boundary.push(boundary_char);
                SwitchAction::SwitchAndReplay {
                    target_layout: v.best_layout,
                    corrected_text: corrected_with_boundary,
                    // Keys, not rendered chars: under-counting is how
                    // word heads get left behind.
                    backspaces: keys.len() + 1,
                    reason: v.reason,
                }
            }
            Some(Verdict::Switch(v)) => {
                low_conf_alt = candidates
                    .iter()
                    .find(|(l, _)| l == &v.best_layout)
                    .map(|(l, t)| (l.clone(), t.clone()));
                SwitchAction::KeepCurrent {
                    reason: format!(
                        "detector confidence {:.2} below threshold {:.2}",
                        v.confidence, snap.engine.confidence_threshold
                    ),
                }
            }
            Some(Verdict::NoOpinion) | None => SwitchAction::KeepCurrent {
                reason: "no detector had an opinion".into(),
            },
        };

        match action {
            SwitchAction::KeepCurrent { reason } => {
                debug!(%reason, "decision: keep current");
                let _ = self.out_tx.send(SwitcherEvent::KeptCurrent { reason });
                // Only for a word that started right after an observed
                // boundary: on a fragment of a longer word a suggestion
                // corrupts it if accepted.
                if started_clean {
                    self.maybe_offer_suggestions(
                        &keys,
                        &current_text,
                        &current_layout,
                        low_conf_alt,
                        &snap,
                    );
                }
            }
            SwitchAction::SwitchAndReplay {
                target_layout,
                corrected_text,
                backspaces,
                reason,
            } => {
                // Original scancodes: re-emitted against the new mapping
                // they produce the corrected glyphs, with no
                // Unicode-compose dance on Wayland.
                let mut replay: Vec<ReplayKey> = keys
                    .iter()
                    .map(|k| ReplayKey {
                        scancode: k.scancode,
                        shift: k.shift,
                    })
                    .collect();
                // Not the key as typed: under the target layout that
                // scancode may well be another character. See
                // `boundary_key_for`.
                let (replay_sc, replay_shift) = boundary_key_for(
                    &self.layouts,
                    &target_layout,
                    boundary_scancode,
                    boundary_shift,
                    boundary_char,
                );
                replay.push(ReplayKey {
                    scancode: replay_sc,
                    shift: replay_shift,
                });
                self.apply_correction(
                    &Correction {
                        from: &current_layout,
                        to: &target_layout,
                        original: &current_text,
                        corrected: &corrected_text,
                        backspaces,
                        reason: &reason,
                        play_sound: snap.general.sound_on_correct,
                        replay_keys: Some(&replay),
                        pointer_click_allowance: 0,
                    },
                    Some((key_rx, buffer)),
                );
            }
        }
    }
}
