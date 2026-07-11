//! Emitting a correction: pre-flight checks, absorbing keystrokes the
//! user lands mid-correction, the delete + replay sequence, and the
//! manual force-switch-last path.

use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;
use poltertype_input::{InputError, KeyDirection, KeyEvent, ReplayKey};
use poltertype_layout::LayoutId;
use tracing::{debug, warn};

use crate::audio::SoundEvent;
use crate::engine::buffer::{KeyKind, WordBuffer, classify};
use crate::engine::consts::PASTE_GUARD;
use crate::engine::enums::SwitcherEvent;
use crate::engine::heuristics::{is_paste_shortcut, is_submission_scancode};
use crate::engine::types::{LastWord, WindowDrain};

use super::engine::SwitcherEngine;

impl SwitcherEngine {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn apply_correction(
        &self,
        from: &LayoutId,
        to: &LayoutId,
        original: &str,
        corrected: &str,
        backspaces: usize,
        reason: &str,
        play_sound: bool,
        replay_keys: Option<&[ReplayKey]>,
        live: Option<(&Receiver<KeyEvent>, &mut WordBuffer)>,
    ) {
        debug!(%from, %to, %original, %corrected, %reason, "applying correction");

        // Pre-flight: confirm the target layout is currently active in
        // the OS BEFORE we touch the user's text.
        //
        // The candidate filter inside `decide()` should already make
        // this impossible for auto-decisions, but keeping the check
        // here closes two more holes:
        //   * `force_switch_last` (manual hotkey) bypasses the
        //     candidate filter and can target any layout in `LayoutDb`.
        //   * Race: settings reload / OS layout list change between
        //     `decide()` and the actual key emission.
        //
        // On query failure we fall through and let `switch_to` surface
        // the original error (still safe — no keystrokes sent yet).
        match self.layout_switcher.list_active() {
            Ok(list) if !list.contains(to) => {
                warn!(
                    target = %to,
                    active = ?list,
                    "target layout not active in OS; aborting correction before any keystrokes"
                );
                return;
            }
            Err(e) => {
                warn!(
                    ?e,
                    "could not list active layouts before correction; continuing"
                );
            }
            _ => {} // active list contains target — proceed.
        }

        // Switch the layout FIRST. Flipping the layout doesn't touch
        // existing text, so if it fails we abort with the user's word
        // fully intact (the old order — backspaces, then switch —
        // destroyed the word whenever the switch failed). It also
        // overlaps the compositor's xkb propagation with the backspace
        // burst, and means any keystrokes the user lands mid-correction
        // already produce glyphs in the layout they intended.
        if let Err(e) = self.layout_switcher.switch_to(to) {
            warn!(?e, target = %to, "layout switch failed; aborting correction before any keystrokes");
            return;
        }

        // ── Absorb: wait for the user's fingers to lift ─────────────
        //
        // Keystrokes the user lands while our backspaces / replay are
        // on the wire physically interleave with them at the
        // compositor — the result is a scrambled word (`рприивіт`)
        // that no amount of after-the-fact counting can fix. So
        // before deleting anything, watch the key stream: as long as
        // presses keep arriving, keep absorbing them into the plan
        // (they are the start of the user's next word — on screen
        // already, in the layout we just switched to). Only when the
        // stream has been quiet for two probes (~50 ms — within a
        // fast typist's inter-key gap) do we start emitting. The
        // absorbed tail is deleted together with the word and
        // re-typed after the boundary, preserving order.
        //
        // If a *boundary* arrives while absorbing, the user finished
        // their next word too — stop there, include it, and re-process
        // it after the correction so the next word gets its own
        // decision. If Enter/Tab (submission) or anything murkier
        // (Backspace, nav, shortcut) arrives, abort the whole
        // correction — nothing has been emitted yet, so the user's
        // text is untouched; we just leave the layout switched and
        // taint the buffer.
        let mut live = live;
        let mut tail: Vec<KeyEvent> = Vec::new();
        let mut resume: Option<KeyEvent> = None;
        let mut suspicious = false;
        if let Some((rx, _)) = live.as_ref() {
            let deadline = Instant::now() + Duration::from_millis(600);
            let mut quiet_probes = 0u8;
            loop {
                let w = self.drain_correction_window(rx);
                tail.extend(w.word_keys);
                suspicious |= w.suspicious;
                if let Some(r) = w.resume {
                    if is_submission_scancode(r.scancode) {
                        suspicious = true;
                    } else {
                        resume = Some(r);
                    }
                    break;
                }
                if suspicious {
                    break;
                }
                if w.saw_user_press {
                    quiet_probes = 0;
                } else {
                    quiet_probes += 1;
                    // ~90 ms of silence. A fast typist's inter-key gap
                    // is ~45-60 ms plus listener lag, so two probes
                    // can land inside a single gap — three cannot.
                    if quiet_probes >= 3 {
                        break;
                    }
                }
                if Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(Duration::from_millis(30));
            }
        }

        if suspicious {
            // Nothing emitted yet — bail out with the text untouched.
            // The buffer can't vouch for the screen any more, though:
            // taint it and drop the manual-switch stash.
            debug!("uncertain keystrokes while preparing correction — aborting untouched");
            if let Some((_, buffer)) = live.as_mut() {
                self.seed_buffer(&tail, buffer);
                buffer.poison();
            }
            *self.last_word.write() = None;
            let _ = self.out_tx.send(SwitcherEvent::LayoutChanged(to.clone()));
            return;
        }

        // ── Delete: word + boundary + absorbed tail (+ its boundary) ─
        //
        // A bounded compensation loop catches the stragglers that
        // still manage to land during the burst itself: each one both
        // soaked up one of our backspaces and must be deleted and
        // re-typed, so it costs exactly one extra backspace either way.
        let mut to_delete = backspaces + tail.len() + usize::from(resume.is_some());
        for round in 0..3 {
            let sent = self.key_emitter.send_backspaces(to_delete);
            self.push_echoes(self.key_emitter.take_emitted());
            if let Err(e) = sent {
                warn!(?e, "send_backspaces failed; aborting correction");
                return;
            }
            let Some((rx, _)) = live.as_ref() else { break };
            // Give raced physical events a moment to travel
            // device → listener thread → our channel.
            std::thread::sleep(Duration::from_millis(12));
            let w = self.drain_correction_window(rx);
            suspicious |= w.suspicious;
            let mut extra = w.word_keys.len();
            tail.extend(w.word_keys);
            if let Some(r) = w.resume {
                if is_submission_scancode(r.scancode) || resume.is_some() {
                    // A second boundary (or a submission key) landed
                    // mid-deletion — too murky to reconstruct.
                    suspicious = true;
                } else {
                    resume = Some(r);
                    extra += 1;
                }
            }
            if extra == 0 {
                break;
            }
            debug!(
                extra,
                round, "user keystrokes raced the deletion; compensating"
            );
            to_delete = extra;
        }

        // ── Replay: word + boundary + tail (+ resume boundary) ──────
        //
        // Prefer replaying the original scancodes against the freshly
        // switched layout (the only path that works in Wayland-native
        // / terminal apps). Backends that have a real Unicode-emit API
        // (`KEYEVENTF_UNICODE`, `CGEventKeyboardSetUnicodeString`)
        // return `Unsupported`; we fall back to `send_text` for them.
        let extra_keys: Vec<ReplayKey> = tail
            .iter()
            .chain(resume.iter())
            .map(|ev| ReplayKey {
                scancode: ev.scancode,
                shift: ev.modifiers.shift,
            })
            .collect();
        let replayed = match replay_keys {
            Some(rk) => {
                let mut full: Vec<ReplayKey> = rk.to_vec();
                full.extend(extra_keys.iter().copied());
                let sent = self.key_emitter.send_keys(&full);
                self.push_echoes(self.key_emitter.take_emitted());
                match sent {
                    Ok(()) => true,
                    Err(InputError::Unsupported(_)) => false,
                    Err(e) => {
                        warn!(?e, "send_keys failed; correction may be partial");
                        return;
                    }
                }
            }
            None => false,
        };
        if !replayed {
            let mut text = corrected.to_owned();
            if let Some(mapping) = self.layouts.get(to) {
                for k in &extra_keys {
                    if let Some(c) = mapping.translate_key(poltertype_types::WordKey {
                        scancode: k.scancode,
                        shift: k.shift,
                        timestamp_ms: 0,
                    }) {
                        text.push(c);
                    }
                }
            }
            let sent = self.key_emitter.send_text(&text);
            self.push_echoes(self.key_emitter.take_emitted());
            if let Err(e) = sent {
                warn!(?e, "send_text failed; correction may be partial");
                return;
            }
        }

        if play_sound {
            self.audio.play(SoundEvent::Correct);
        }
        let _ = self.out_tx.send(SwitcherEvent::Corrected {
            from_layout: from.clone(),
            to_layout: to.clone(),
            original_text: original.to_owned(),
            corrected_text: corrected.to_owned(),
            reason: reason.to_owned(),
        });
        let _ = self.out_tx.send(SwitcherEvent::LayoutChanged(to.clone()));

        // ── Settle & seed ───────────────────────────────────────────
        if let Some((rx, buffer)) = live {
            // Wait out our own echoes before letting the run loop
            // resume. Rationale: consume_echo matches by scancode, so
            // while the queue is non-empty a REAL user press of the
            // same scancode we just replayed would be swallowed as an
            // echo (very possible right after a correction — the next
            // word often starts with the same letters). Draining here,
            // while the user is still inside the pause the absorb gate
            // verified, empties the queue in one keyd round-trip;
            // anything the user types afterwards can't be mistaken for
            // an echo. Bounded: backends whose echoes never come back
            // through the listener (Windows / macOS tag them injected
            // instead) just wait out the deadline once — after
            // emission, so the user never sees the latency.
            let mut post_tail: Vec<KeyEvent> = Vec::new();
            let mut post_resume: Option<KeyEvent> = None;
            let settle_deadline = Instant::now() + Duration::from_millis(400);
            loop {
                let w = self.drain_correction_window(rx);
                post_tail.extend(w.word_keys);
                suspicious |= w.suspicious;
                if let Some(r) = w.resume {
                    if post_resume.is_some() || is_submission_scancode(r.scancode) {
                        suspicious = true;
                    } else {
                        post_resume = Some(r);
                    }
                }
                if !self.echo_pending() || Instant::now() >= settle_deadline {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }

            if suspicious {
                // Something we couldn't attribute cleanly landed
                // mid-correction. The screen state is uncertain until
                // the next boundary — track nothing, correct nothing.
                buffer.abandon();
                buffer.poison();
                *self.last_word.write() = None;
            } else {
                // Chronological re-assembly of everything the user
                // typed while we were busy: the absorbed tail (word
                // in progress), its boundary (routed through the
                // normal pipeline so that word gets its own decision
                // — usually "keep", it was typed post-switch), then
                // whatever arrived after the replay.
                self.seed_buffer(&tail, buffer);
                if let Some(r) = resume {
                    self.handle_key(r, buffer, rx);
                }
                self.seed_buffer(&post_tail, buffer);
                if let Some(r) = post_resume {
                    self.handle_key(r, buffer, rx);
                }
            }
        }
    }

    /// Feed absorbed keystrokes into the buffer as the in-progress
    /// word (they are on screen after the corrected boundary).
    fn seed_buffer(&self, tail: &[KeyEvent], buffer: &mut WordBuffer) {
        for ev in tail {
            let letter = self
                .layouts
                .is_letter_in_any_layout(ev.scancode, ev.modifiers.shift);
            let produced = if letter {
                None
            } else {
                self.translate_via_current_layout(ev.scancode, ev.modifiers.shift)
            };
            let _ = buffer.feed(*ev, produced, letter);
        }
    }

    /// Drain everything currently pending on the listener channel,
    /// swallowing our own echoes. Collects the plain word-key presses
    /// the user managed to type while a correction was in flight;
    /// stops at the first boundary press (`resume` — the user finished
    /// their next word too). Anything murkier (Backspace, nav, click,
    /// shortcut) sets `suspicious`.
    fn drain_correction_window(&self, rx: &Receiver<KeyEvent>) -> WindowDrain {
        let mut out = WindowDrain::default();
        while let Ok(ev) = rx.try_recv() {
            if self.consume_echo(&ev) {
                continue;
            }
            if ev.injected || ev.direction != KeyDirection::Press {
                continue;
            }
            out.saw_user_press = true;
            if is_paste_shortcut(&ev) {
                *self.paste_guard_until.write() = Instant::now() + PASTE_GUARD;
            }
            if ev.modifiers.is_command() {
                out.suspicious = true;
                break;
            }
            let letter = self
                .layouts
                .is_letter_in_any_layout(ev.scancode, ev.modifiers.shift);
            let produced = if letter {
                None
            } else {
                self.translate_via_current_layout(ev.scancode, ev.modifiers.shift)
            };
            match classify(ev.scancode, produced, letter) {
                KeyKind::Word => out.word_keys.push(ev),
                KeyKind::Discard => {}
                KeyKind::Boundary => {
                    out.resume = Some(ev);
                    break;
                }
                // Backspace / nav / click mid-correction — can't
                // reconstruct where it landed.
                KeyKind::Backspace | KeyKind::EndAndDiscard => {
                    out.suspicious = true;
                    break;
                }
            }
        }
        out
    }

    pub(super) fn force_switch_last(
        &self,
        last: LastWord,
        buffer: &mut WordBuffer,
        key_rx: &Receiver<KeyEvent>,
    ) {
        // Pick the most plausible alternate layout — in v0.1 with two
        // layouts, "the other one" is fine. Generalisation will
        // re-run the detector pipeline with `min_advantage = 0`.
        let other = self.layouts.ids().find(|id| **id != last.layout).cloned();
        let Some(target) = other else {
            warn!("only one layout known; can't force-switch");
            return;
        };
        let target_mapping = match self.layouts.get(&target) {
            Some(m) => m,
            None => {
                warn!(%target, "target layout not in DB");
                return;
            }
        };
        let mut corrected = target_mapping.translate_buffer(&last.keys);
        corrected.push(last.boundary_char);
        // Replay the boundary the user actually typed — except
        // Enter/Tab, where a re-press would submit the line / move
        // focus; substitute a space for those.
        let (boundary_sc, boundary_shift) = match last.boundary_scancode {
            0x1C | 0x0F | 0x60 => (0x39, false),
            sc => (sc, last.boundary_shift),
        };
        let mut replay: Vec<ReplayKey> = last
            .keys
            .iter()
            .map(|k| ReplayKey {
                scancode: k.scancode,
                shift: k.shift,
            })
            .collect();
        replay.push(ReplayKey {
            scancode: boundary_sc,
            shift: boundary_shift,
        });
        self.apply_correction(
            &last.layout,
            &target,
            &last.rendered,
            &corrected,
            last.keys.len() + 1,
            "manual switch-last hotkey",
            true,
            Some(&replay),
            Some((key_rx, buffer)),
        );
    }
}
