//! Per-completed-word decision: candidate filtering, smart-command
//! lookup, the pre-decision filters, and the detector pipeline.

use crossbeam_channel::Receiver;
use poltertype_detect::{Verdict, looks_like_code_token};
use poltertype_input::{KeyEvent, ReplayKey};
use poltertype_layout::LayoutId;
use poltertype_types::SwitchAction;
use tracing::{debug, warn};

use crate::commands::find_matching_command;
use crate::engine::buffer::WordBuffer;
use crate::engine::enums::SwitcherEvent;
use crate::engine::heuristics::{
    app_is_disabled, is_layout_eligible, is_structural_boundary, is_submission_boundary,
    looks_like_all_caps, render_for_code_check,
};
use crate::engine::types::LastWord;

use super::engine::SwitcherEngine;

impl SwitcherEngine {
    /// Best-effort current-layout translate. Returns `None` if we
    /// can't query the OS or the scancode isn't in the mapping
    /// table — both are normal for control / OEM keys.
    pub(super) fn translate_via_current_layout(&self, scancode: u32, shift: bool) -> Option<char> {
        let current = self.layout_switcher.current().ok()?;
        let mapping = self.layouts.get(&current)?;
        mapping.translate_key(poltertype_types::WordKey {
            scancode,
            shift,
            timestamp_ms: 0,
        })
    }

    pub(super) fn decide(
        &self,
        buffer: &mut WordBuffer,
        boundary_scancode: u32,
        boundary_shift: bool,
        key_rx: &Receiver<KeyEvent>,
    ) {
        let snap = self.settings.snapshot();
        let keys = buffer.completed().to_vec();
        if keys.is_empty() {
            return;
        }
        // Note: no global min_word_length gate here. Each detector
        // decides on its own — the dictionary detector wants to see
        // single-letter prepositions; word-plausibility self-filters
        // to ≥3 letters.

        let current_layout = match self.layout_switcher.current() {
            Ok(l) => l,
            Err(e) => {
                warn!(?e, "could not query current layout; skipping decision");
                return;
            }
        };

        // Filter the candidate set down to layouts the engine can
        // actually switch to. Three layers, all must pass:
        //
        //   1. `[languages].active` from settings — empty means "every
        //      loaded layout".
        //   2. `[languages].ignored` from settings — always vetoes.
        //   3. The OS's current active-layouts list — anything not
        //      installed/enabled in the OS is unreachable. Including it
        //      here previously caused the detector to pick e.g. `fr-FR`
        //      for `http` even when the user only has en-US / ru-RU /
        //      uk-UA installed — `switch_to()` would then reject it,
        //      and because backspaces had already been emitted the word
        //      was simply destroyed. Filtering here means the detector
        //      never sees the unreachable layout to begin with.
        //
        // If the OS query fails we fail-open (skip layer 3), matching
        // the previous behaviour. The pre-flight check inside
        // `apply_correction` is the second line of defence.
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

        // ---- Stash the rendered current text early; "force switch
        // last" uses it whether or not auto-decision proceeds. ----
        let current_text = candidates
            .iter()
            .find(|(l, _)| l == &current_layout)
            .map(|(_, t)| t.clone())
            .unwrap_or_default();

        // Resolve boundary scancode → character (even if we skip the
        // decision below, store it so the manual hotkey works).
        let boundary_char = self
            .layouts
            .get(&current_layout)
            .and_then(|m| {
                m.translate_key(poltertype_types::WordKey {
                    scancode: boundary_scancode,
                    shift: boundary_shift,
                    timestamp_ms: 0,
                })
            })
            .or(match boundary_scancode {
                0x39 => Some(' '),
                0x1C | 0x60 => Some('\n'), // Enter / numpad Enter
                0x0F => Some('\t'),
                _ => None,
            })
            .unwrap_or(' ');

        *self.last_word.write() = Some(LastWord {
            keys: keys.clone(),
            rendered: current_text.clone(),
            layout: current_layout.clone(),
            boundary_char,
            boundary_scancode,
            boundary_shift,
        });

        // ---- Smart commands (text triggers) ----
        //
        // Consult the user's `[[commands]]` list BEFORE the auto-
        // switch filters: text expansion is a direct user
        // intent ("I typed `anrl ` because I want it expanded"),
        // not a guess the engine is making, so the structural-
        // boundary / disabled-app / identifier filters don't apply.
        //
        // The match is on the rendering in the CURRENT layout —
        // typing `anrl ` under en-US matches `anrl`, but typing the
        // same physical keys under uk-UA produces a Cyrillic
        // rendering that won't match (which is the right behaviour:
        // an English acronym typed accidentally in Ukrainian layout
        // should go through normal layout-correction, not
        // expansion).
        let focused_basename = self.focus_tracker.focused_exe().and_then(|exe| {
            std::path::Path::new(&exe)
                .file_name()
                .and_then(|f| f.to_str())
                .map(str::to_owned)
        });
        if let Some(cmd) =
            find_matching_command(&snap.commands, &current_text, focused_basename.as_deref())
        {
            // Erase one on-screen character per buffered key plus the
            // boundary. Counting keys (not rendered chars) survives
            // scancodes our mapping table can't render — the screen
            // still shows a character for those.
            self.dispatch_smart_command(cmd, keys.len() + 1, boundary_char);
            // The trigger text no longer exists on screen — the word
            // must not be re-openable via backspace.
            buffer.forget_completed();
            return;
        }

        // ---- Pre-decision filters (auto-switch only) ----
        //
        // Filters apply *only* to automatic decisions — the manual
        // switch-last hotkey calls force_switch_last directly and
        // bypasses all of them. That's the dev-friendly contract:
        // we stay quiet by default in code / URL / path contexts,
        // but if the user explicitly hits the hotkey we always do
        // the switch.

        // Filter 0a: submission / navigation boundary (Enter, Tab).
        // Re-emitting one of these as part of the correction replay
        // executes a shell command, sends a chat message, or fires a
        // completion — and the correction fires too late to be useful
        // anyway (the line is already submitted). Stay out of it.
        if is_submission_boundary(boundary_char) {
            debug!(
                token = %current_text,
                "skipping auto-switch: submission boundary (Enter/Tab)"
            );
            let _ = self.out_tx.send(SwitcherEvent::KeptCurrent {
                reason: format!(
                    "submission boundary after `{current_text}` — not re-emitting Enter/Tab"
                ),
            });
            return;
        }

        // Filter 0: structural boundary character. If the user
        // ended the word with `:` / `/` / `\` / `@` / `=` / `#` /
        // `&` then they're typing a URL / path / email / config
        // expression / code, NOT prose. Switching `http` to `реез`
        // because they just typed `:` would corrupt the URL they're
        // half-way through. Skip.
        if is_structural_boundary(boundary_char) {
            debug!(
                token = %current_text,
                boundary = %boundary_char,
                "skipping auto-switch: structural boundary"
            );
            let _ = self.out_tx.send(SwitcherEvent::KeptCurrent {
                reason: format!(
                    "structural boundary `{boundary_char}` after `{current_text}` — likely URL / path / email / code"
                ),
            });
            return;
        }

        // Filter 0c: ALL-CAPS word (held Shift / Caps Lock through
        // the whole token). The user typing `URL`, `HTTP`, `API`,
        // `ССЫЛКА` is being deliberate — they're not "in the wrong
        // layout", they're spelling out an abbreviation. Auto-
        // switching here is the classic glitchy case: the all-caps
        // token often happens to render as something letter-like in
        // the other layout, the detector takes the bait, and the
        // user watches their abbreviation get replaced with
        // gibberish.
        //
        // Held-Shift catches the case on every backend. Caps Lock
        // catches it on Linux/Wayland (the listener XORs caps into
        // the shift bit before the engine sees the event). Windows /
        // macOS don't yet fold Caps Lock into the modifier — a
        // separate fix at the per-OS listener level — but the
        // held-Shift variant covers most ALL-CAPS typing there too.
        //
        // The manual switch-last hotkey still works on these buffers:
        // `last_word` was stashed above before any filter ran.
        if snap.engine.suppress_for_all_caps && looks_like_all_caps(&current_text) {
            debug!(
                token = %current_text,
                "skipping auto-switch: word is ALL CAPS (likely abbreviation)"
            );
            let _ = self.out_tx.send(SwitcherEvent::KeptCurrent {
                reason: format!(
                    "`{current_text}` is ALL CAPS — likely an abbreviation, not a wrong-layout word"
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

        // Filter 2: token looks like an identifier (camelCase /
        // snake_case / letter+digit / code punct).
        //
        // We feed `looks_like_code_token` a *cleaned* rendering that
        // strips cross-layout artifacts — characters whose scancode is
        // a letter in some other layout but renders as punctuation
        // under the current one. Without this, typing a Ukrainian word
        // containing `ж` (scancode 0x27) under en-US produces a `;`
        // mid-string and the heuristic would (wrongly) call the buffer
        // "code". See `render_for_code_check` for details.
        let token_for_code_check =
            render_for_code_check(&keys, &current_layout, &self.layouts, &current_text);
        if snap.engine.suppress_in_identifiers && looks_like_code_token(&token_for_code_check) {
            debug!(
                token = %current_text,
                cleaned = %token_for_code_check,
                "skipping auto-switch: looks like code identifier"
            );
            let _ = self.out_tx.send(SwitcherEvent::KeptCurrent {
                reason: format!("token `{current_text}` looks like an identifier"),
            });
            return;
        }

        let ctx = poltertype_detect::DetectionContext {
            current_layout: &current_layout,
            candidates: &candidates,
            recent_context: "",
        };

        // Run detectors in priority order. The first non-NoOpinion
        // verdict wins — including a `Keep` veto, which short-circuits
        // the rest of the pipeline.
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
                // The boundary key has already been delivered to the
                // focused app, so we have to delete it too and re-emit
                // a copy after the corrected word.
                let mut corrected_with_boundary = target_text;
                corrected_with_boundary.push(boundary_char);
                SwitchAction::SwitchAndReplay {
                    target_layout: v.best_layout,
                    corrected_text: corrected_with_boundary,
                    // One on-screen character per buffered key + the
                    // boundary. Keys (not rendered chars): a scancode
                    // missing from our mapping table still produced a
                    // character on screen, and under-counting here is
                    // exactly how word heads get left behind.
                    backspaces: keys.len() + 1,
                    reason: v.reason,
                }
            }
            Some(Verdict::Switch(v)) => SwitchAction::KeepCurrent {
                reason: format!(
                    "detector confidence {:.2} below threshold {:.2}",
                    v.confidence, snap.engine.confidence_threshold
                ),
            },
            Some(Verdict::NoOpinion) | None => SwitchAction::KeepCurrent {
                reason: "no detector had an opinion".into(),
            },
        };

        match action {
            SwitchAction::KeepCurrent { reason } => {
                debug!(%reason, "decision: keep current");
                let _ = self.out_tx.send(SwitcherEvent::KeptCurrent { reason });
            }
            SwitchAction::SwitchAndReplay {
                target_layout,
                corrected_text,
                backspaces,
                reason,
            } => {
                // Build the replay sequence: original word scancodes
                // + the boundary key. After the layout flips, emitting
                // these same scancodes against the new xkb mapping
                // produces the corrected glyphs — no Unicode-compose
                // dance needed on Wayland.
                let mut replay: Vec<ReplayKey> = keys
                    .iter()
                    .map(|k| ReplayKey {
                        scancode: k.scancode,
                        shift: k.shift,
                    })
                    .collect();
                replay.push(ReplayKey {
                    scancode: boundary_scancode,
                    shift: boundary_shift,
                });
                self.apply_correction(
                    &current_layout,
                    &target_layout,
                    &current_text,
                    &corrected_text,
                    backspaces,
                    &reason,
                    snap.general.sound_on_correct,
                    Some(&replay),
                    Some((key_rx, buffer)),
                );
            }
        }
    }
}
