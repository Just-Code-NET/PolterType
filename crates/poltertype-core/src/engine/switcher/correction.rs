//! Emitting a correction: pre-flight checks, absorbing keystrokes the
//! user lands mid-correction, the delete + replay sequence, and the
//! manual force-switch-last path.

use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;
use poltertype_input::{InputError, KeyDirection, KeyEvent, ReplayKey};
use poltertype_layout::LayoutId;
use poltertype_types::logsafe;
use tracing::{debug, warn};

use crate::audio::SoundEvent;
use crate::engine::buffer::{KeyKind, WordBuffer, classify};
use crate::engine::consts::{
    HELD_FLUSH, HELD_FLUSH_QUIET_PROBES, INTRUSION_PROBES, INTRUSION_QUIET_PROBES,
    INTRUSION_REPAIRS, LAYOUT_SETTLE, PASTE_GUARD, POST_EMIT_LAG, SC_BACKSPACE, SC_SPACE,
};
use crate::engine::enums::{DictionaryAddOrigin, SwitcherEvent};
use crate::engine::heuristics::{boundary_key_for, is_paste_shortcut, is_submission_scancode};
use crate::engine::types::{Correction, HeldKeys, LastWord, WindowDrain};

use super::engine::SwitcherEngine;

/// Shortest word an undone correction may teach the dictionary. Same
/// three-letter floor the suggestion tooltip uses, for the same
/// reason: below it the engine is not working from the FST at all.
const MIN_LEARNED_LETTERS: usize = 3;

impl SwitcherEngine {
    /// Type out keystrokes the key gate held back, by whichever emit
    /// path this backend has.
    ///
    /// `send_keys` replays raw scancodes; backends that answer
    /// `Unsupported` fall back to `send_text`. **Never give up here** —
    /// these keys were already swallowed from the application, so
    /// dropping them loses the user's typing outright. See
    /// `docs/ARCHITECTURE.md` § Key gate.
    ///
    /// Keystrokes that are not characters (Backspace, arrows, Esc) have
    /// no rendering in any layout and are dropped; bounded by one burst.
    fn emit_held_keys(&self, keys: &[ReplayKey], to: &LayoutId) -> Result<(), InputError> {
        let sent = self.key_emitter.send_keys(keys);
        self.push_echoes(self.key_emitter.take_emitted());
        match sent {
            Err(InputError::Unsupported(_)) => {}
            other => return other,
        }

        let mapping = self.layouts.get(to);
        let mut text = String::new();
        let mut dropped = 0usize;

        for k in keys {
            // Backspace goes out as a keypress, and in its place in
            // the sequence — emitting it after the rest would eat the
            // wrong character.
            if k.scancode == SC_BACKSPACE {
                self.flush_text(&mut text)?;
                let sent = self.key_emitter.send_backspaces(1);
                self.push_echoes(self.key_emitter.take_emitted());
                sent?;
                continue;
            }
            // Space is handled before the overlay because no overlay
            // has it: a layout describes the 46 character keys. It is
            // also the likeliest key to be held, being the boundary
            // that triggers most corrections.
            //
            // Enter and Tab are deliberately absent — replaying them
            // submits a line or moves focus.
            let c = if k.scancode == SC_SPACE {
                Some(' ')
            } else {
                mapping.and_then(|m| {
                    m.translate_key(poltertype_types::WordKey {
                        scancode: k.scancode,
                        shift: k.shift,
                        timestamp_ms: 0,
                    })
                })
            };
            match c {
                Some(c) => text.push(c),
                None => dropped += 1,
            }
        }
        self.flush_text(&mut text)?;

        if dropped > 0 {
            // Counts only — never the characters, same rule as the rest
            // of this path.
            debug!(
                dropped,
                "held keys that are neither text nor Backspace could not be replayed"
            );
        }
        Ok(())
    }

    /// Emit whatever text has accumulated, and empty the buffer.
    fn flush_text(&self, text: &mut String) -> Result<(), InputError> {
        if text.is_empty() {
            return Ok(());
        }
        let sent = self.key_emitter.send_text(text);
        self.push_echoes(self.key_emitter.take_emitted());
        text.clear();
        sent
    }

    /// Returns `true` once keystrokes were actually emitted (delete +
    /// replay happened, however imperfectly) — `false` means the
    /// correction aborted with the user's text untouched.
    ///
    /// `live` is the running key stream and word buffer, present
    /// whenever there is a session to absorb raced keystrokes into and
    /// `None` in the tests that only assert what was emitted.
    pub(super) fn apply_correction(
        &self,
        c: &Correction<'_>,
        live: Option<(&Receiver<KeyEvent>, &mut WordBuffer)>,
    ) -> bool {
        let &Correction {
            from,
            to,
            original,
            corrected,
            backspaces,
            reason,
            play_sound,
            replay_keys,
            pointer_click_allowance,
        } = c;
        debug!(
            %from,
            %to,
            original = %logsafe::redact_word(original),
            corrected = %logsafe::redact_word(corrected),
            %reason,
            "applying correction"
        );

        // A same-layout replacement (spelling suggestion) has no layout
        // to flip; everything switch-related below is keyed off this.
        let switching = from != to;

        // When the layout flip happened — the replay must not outrun
        // the compositor's xkb propagation. See `LAYOUT_SETTLE`.
        let mut switched_at: Option<Instant> = None;

        // Pre-flight the target layout BEFORE touching the user's text.
        // `decide()` already filters candidates, but `force_switch_last`
        // bypasses that filter, and settings or the OS layout list can
        // change in between. On query failure fall through and let
        // `switch_to` surface the error — still safe, nothing sent yet.
        if switching {
            match self.layout_switcher.list_active() {
                Ok(list) if !list.contains(to) => {
                    warn!(
                        target = %to,
                        active = ?list,
                        "target layout not active in OS; aborting correction before any keystrokes"
                    );
                    return false;
                }
                Err(e) => {
                    warn!(
                        ?e,
                        "could not list active layouts before correction; continuing"
                    );
                }
                _ => {} // active list contains target — proceed.
            }

            // Layout first: a failed switch then leaves the word
            // intact. See `docs/ARCHITECTURE.md` § The correction path.
            if let Err(e) = self.layout_switcher.switch_to(to) {
                warn!(?e, target = %to, "layout switch failed; aborting correction before any keystrokes");
                return false;
            }
            switched_at = Some(Instant::now());
        }

        // ── Absorb: wait for the user's fingers to lift ─────────────
        //
        // Keystrokes landing while our burst is on the wire interleave
        // with it at the compositor, and counting cannot fix that. So
        // fold arriving presses into the plan until the stream has been
        // empty three times running, then emit. A boundary means the
        // user finished their next word too — include it and re-process
        // it. A submission or anything murkier aborts, untouched.
        let mut live = live;
        let mut click_allowance = pointer_click_allowance;
        let mut tail: Vec<KeyEvent> = Vec::new();
        let mut resume: Option<KeyEvent> = None;
        let mut suspicious = false;
        if let Some((rx, _)) = live.as_ref() {
            let deadline = Instant::now() + Duration::from_millis(600);
            let mut quiet_probes = 0u8;
            loop {
                let w = self.drain_correction_window(rx, &mut click_allowance);
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
                    // Three empty probes, two 30 ms sleeps: ~60 ms,
                    // past a fast typist's inter-key gap. A correction
                    // fired by a chord waits for that chord to come up
                    // as well — under a held `Ctrl` our replay produces
                    // shortcuts, and releasing on our side is not
                    // enough where a remapper keeps its own idea of
                    // what is down. The deadline below bounds the wait.
                    if quiet_probes >= 3 && !self.modifiers_held() {
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
            // Nothing emitted — bail out untouched. The buffer can no
            // longer vouch for the screen: taint it and drop the stash.
            debug!("uncertain keystrokes while preparing correction — aborting untouched");
            if let Some((_, buffer)) = live.as_mut() {
                self.seed_buffer(&tail, buffer);
                buffer.poison();
            }
            *self.last_word.write() = None;
            if switching {
                let _ = self.out_tx.send(SwitcherEvent::LayoutChanged(to.clone()));
            }
            return false;
        }

        // Wait out xkb propagation here rather than just before the
        // replay, so it cannot widen the gap between our last look at
        // the key stream and our first emitted key.
        if let Some(t) = switched_at {
            let since = t.elapsed();
            if since < LAYOUT_SETTLE {
                std::thread::sleep(LAYOUT_SETTLE - since);
            }
        }

        // ── Emit: delete → replay ───────────────────────────────────
        //
        // The gate holds the user's keys back for the length of the
        // burst; where it cannot run we probe for an intrusion
        // afterwards instead. Release whatever the user is holding
        // first — a replay under a held Ctrl produces shortcuts, not
        // text, and the correction appears not to happen.
        let holding = *self.held_modifiers.read();
        if holding.control || holding.shift || holding.alt || holding.meta {
            debug!(?holding, "releasing held modifiers before emitting");
            if let Err(e) = self.key_emitter.release_modifiers(holding) {
                warn!(
                    ?e,
                    "could not release held modifiers; replay may be swallowed"
                );
            }
            self.push_echoes(self.key_emitter.take_emitted());
        }

        let mut held = HeldKeys::acquire(&self.key_gate);
        let mut repairs_left = INTRUSION_REPAIRS;
        let mut to_delete = backspaces + tail.len() + usize::from(resume.is_some());
        loop {
            // ── Delete: word + boundary + absorbed tail ─────────────
            //
            // Bounded compensation loop: a straggler landing during the
            // burst both soaks up one backspace and needs deleting, so
            // it costs exactly one extra either way. Exits on an empty
            // probe, with the replay immediately after.
            for round in 0..3 {
                let sent = self.key_emitter.send_backspaces(to_delete);
                self.push_echoes(self.key_emitter.take_emitted());
                if let Err(e) = sent {
                    warn!(?e, "send_backspaces failed; aborting correction");
                    return false;
                }
                let Some((rx, _)) = live.as_ref() else { break };
                // Held keyboard: nothing of the user's reached the
                // screen, so there is nothing to compensate for.
                if held.active() {
                    break;
                }
                // Give raced physical events time to travel
                // device → listener thread → our channel.
                std::thread::sleep(POST_EMIT_LAG);
                let w = self.drain_correction_window(rx, &mut click_allowance);
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

            // ── Replay: word + boundary + tail (+ resume boundary) ──
            //
            // Original scancodes against the freshly switched layout —
            // the only path that works in Wayland-native and terminal
            // apps. Unicode-emit backends answer `Unsupported` and get
            // `send_text`.
            let extra_keys: Vec<ReplayKey> = tail
                .iter()
                .chain(resume.iter())
                .map(|ev| ReplayKey {
                    scancode: ev.scancode,
                    shift: ev.modifiers.shift,
                })
                .collect();
            let mut emitted = 0usize;
            let replayed = match replay_keys {
                Some(rk) => {
                    let mut full: Vec<ReplayKey> = rk.to_vec();
                    full.extend(extra_keys.iter().copied());
                    emitted = full.len();
                    let sent = self.key_emitter.send_keys(&full);
                    self.push_echoes(self.key_emitter.take_emitted());
                    match sent {
                        Ok(()) => true,
                        Err(InputError::Unsupported(_)) => false,
                        Err(e) => {
                            warn!(?e, "send_keys failed; correction may be partial");
                            return false;
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
                emitted = text.chars().count();
                let sent = self.key_emitter.send_text(&text);
                self.push_echoes(self.key_emitter.take_emitted());
                if let Err(e) = sent {
                    warn!(?e, "send_text failed; correction may be partial");
                    return false;
                }
            }

            let Some((rx, _)) = live.as_ref() else {
                break;
            };

            // ── Flush: type out what the gate held back ─────────────
            //
            // These keys never reached the application, so they simply
            // go on the end in press order. Keep going while the user
            // keeps typing, up to a bound.
            if held.active() {
                let flush_deadline = Instant::now() + HELD_FLUSH;
                // One empty sweep is shorter than an inter-key gap;
                // letting go on it drops whatever is pressed in the hole
                // between the sweep and the actual ungrab.
                let mut quiet = 0u8;
                loop {
                    std::thread::sleep(POST_EMIT_LAG);
                    let w = self.drain_correction_window(rx, &mut click_allowance);
                    let mut pending: Vec<ReplayKey> = w
                        .word_keys
                        .iter()
                        .map(|ev| ReplayKey {
                            scancode: ev.scancode,
                            shift: ev.modifiers.shift,
                        })
                        .collect();
                    suspicious |= w.suspicious;
                    tail.extend(w.word_keys);
                    if let Some(r) = w.resume {
                        pending.push(ReplayKey {
                            scancode: r.scancode,
                            shift: r.modifiers.shift,
                        });
                        if is_submission_scancode(r.scancode) || resume.is_some() {
                            suspicious = true;
                        } else {
                            resume = Some(r);
                        }
                    }
                    // Backspace / arrows / Esc were swallowed too, and
                    // they are the user editing — type them out after
                    // our text, where they would have landed. A shortcut
                    // needs modifiers we cannot reproduce and arrives as
                    // `None`; all we can do is stop holding at once.
                    if let Some(s) = w.stopper {
                        pending.push(ReplayKey {
                            scancode: s.scancode,
                            shift: s.modifiers.shift,
                        });
                    }
                    if pending.is_empty() {
                        quiet += 1;
                    } else {
                        quiet = 0;
                        debug!(
                            count = pending.len(),
                            "typing out keystrokes the gate held back"
                        );
                        if let Err(e) = self.emit_held_keys(&pending, to) {
                            warn!(?e, "flushing held keystrokes failed");
                            break;
                        }
                    }
                    if quiet >= HELD_FLUSH_QUIET_PROBES
                        || suspicious
                        || Instant::now() >= flush_deadline
                    {
                        break;
                    }
                }
                // Letting go is synchronous: everything already on the
                // stream is ours to type out, everything after reaches
                // the application by itself. One last sweep for the
                // stragglers on our side of that line.
                held.release();
                let w = self.drain_correction_window(rx, &mut click_allowance);
                let mut last: Vec<ReplayKey> = w
                    .word_keys
                    .iter()
                    .map(|ev| ReplayKey {
                        scancode: ev.scancode,
                        shift: ev.modifiers.shift,
                    })
                    .collect();
                suspicious |= w.suspicious;
                tail.extend(w.word_keys);
                if let Some(r) = w.resume {
                    last.push(ReplayKey {
                        scancode: r.scancode,
                        shift: r.modifiers.shift,
                    });
                    if is_submission_scancode(r.scancode) || resume.is_some() {
                        suspicious = true;
                    } else {
                        resume = Some(r);
                    }
                }
                if let Some(st) = w.stopper {
                    last.push(ReplayKey {
                        scancode: st.scancode,
                        shift: st.modifiers.shift,
                    });
                }
                if !last.is_empty() {
                    debug!(count = last.len(), "typing out the last held keystrokes");
                    // Not `send_keys` directly — see `emit_held_keys`.
                    if let Err(e) = self.emit_held_keys(&last, to) {
                        warn!(?e, "flushing the last held keystrokes failed");
                    }
                }
                break;
            }

            // ── Intrusion probe (gate unavailable) ──────────────────
            //
            // Anything on the wire now landed inside the text we just
            // typed. The position is unknown, the character count is
            // not, so erase that many plus the intruders and retype.
            // The repair is itself a burst, so wait for a pause; if none
            // comes, leave the screen as it is and stop vouching for it.
            if suspicious {
                break;
            }
            let mut intruders = 0usize;
            let mut quiet = 0u8;
            let mut probes = 0u8;
            loop {
                std::thread::sleep(POST_EMIT_LAG);
                let w = self.drain_correction_window(rx, &mut click_allowance);
                let saw_press = w.saw_user_press;
                suspicious |= w.suspicious;
                intruders += w.word_keys.len();
                tail.extend(w.word_keys);
                if let Some(r) = w.resume {
                    if is_submission_scancode(r.scancode) || resume.is_some() {
                        suspicious = true;
                    } else {
                        resume = Some(r);
                        intruders += 1;
                    }
                }
                if suspicious {
                    break;
                }
                if saw_press {
                    quiet = 0;
                } else {
                    quiet += 1;
                }
                // Clean burst: one empty probe settles it.
                probes += 1;
                if intruders == 0 || quiet >= INTRUSION_QUIET_PROBES || probes >= INTRUSION_PROBES {
                    break;
                }
            }
            if intruders == 0 {
                break;
            }
            if suspicious || repairs_left == 0 || quiet < INTRUSION_QUIET_PROBES {
                // Budget spent, or no pause ever came. The screen holds
                // something we cannot place — track nothing.
                suspicious = true;
                break;
            }
            repairs_left -= 1;
            debug!(
                intruders,
                emitted, "keystrokes landed inside the replay; re-emitting in typed order"
            );
            to_delete = emitted + intruders;
        }

        if play_sound {
            self.audio.play(SoundEvent::Correct);
        }
        // Layout-correction events only; a same-layout replacement
        // announces itself via `SuggestionApplied` from its own caller.
        if switching {
            let _ = self.out_tx.send(SwitcherEvent::Corrected {
                from_layout: from.clone(),
                to_layout: to.clone(),
                original_text: original.to_owned(),
                corrected_text: corrected.to_owned(),
                reason: reason.to_owned(),
            });
            let _ = self.out_tx.send(SwitcherEvent::LayoutChanged(to.clone()));
            // The stashed word now reads differently than it was typed,
            // so record where we took it — the manual hotkey undoes a
            // correction rather than re-applying one. Here rather than
            // in `decide`, because only now is it actually on screen.
            if let Some(last) = self.last_word.write().as_mut() {
                last.corrected_to = Some(to.clone());
            }
        }

        // ── Settle & seed ───────────────────────────────────────────
        if let Some((rx, buffer)) = live {
            // Drain our own echoes before the run loop resumes:
            // `consume_echo` matches by scancode, so a real press of a
            // scancode we just replayed would be swallowed while the
            // queue is non-empty. Bounded, because backends that tag
            // echoes injected never send them back at all.
            let mut post_tail: Vec<KeyEvent> = Vec::new();
            let mut post_resume: Option<KeyEvent> = None;
            let settle_deadline = Instant::now() + Duration::from_millis(400);
            loop {
                let w = self.drain_correction_window(rx, &mut click_allowance);
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
                // Something unattributable landed mid-correction. The
                // screen is uncertain until the next boundary.
                buffer.abandon();
                buffer.poison();
                *self.last_word.write() = None;
            } else {
                // Chronological re-assembly of what the user typed
                // while we were busy: absorbed tail, its boundary
                // (through the normal pipeline, so that word gets its
                // own decision), then whatever arrived after the replay.
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
        true
    }

    /// Feed absorbed keystrokes into the buffer as the in-progress
    /// word (they are on screen after the corrected boundary).
    fn seed_buffer(&self, tail: &[KeyEvent], buffer: &mut WordBuffer) {
        for ev in tail {
            let _ = self.feed_buffer(*ev, buffer);
        }
    }

    /// Drain everything pending on the listener channel, swallowing our
    /// own echoes. Collects the plain word-key presses the user managed
    /// to type during a correction and stops at the first boundary
    /// press (`resume`). Anything murkier sets `suspicious`;
    /// `click_allowance` pointer presses are swallowed benignly.
    fn drain_correction_window(
        &self,
        rx: &Receiver<KeyEvent>,
        click_allowance: &mut usize,
    ) -> WindowDrain {
        let mut out = WindowDrain::default();
        while let Ok(ev) = rx.try_recv() {
            if self.consume_echo(&ev) {
                continue;
            }
            if !ev.injected {
                // Releases are dropped below, but they are the only
                // sign the triggering chord has been let go of — see
                // `modifiers_held`.
                *self.held_modifiers.write() = ev.modifiers;
            }
            if ev.injected || ev.direction != KeyDirection::Press {
                continue;
            }
            if ev.scancode == poltertype_types::SC_POINTER_BUTTON && *click_allowance > 0 {
                // The click that accepted the tooltip, echoing through
                // the key stream — it never reached the app below.
                *click_allowance -= 1;
                continue;
            }
            out.saw_user_press = true;
            if is_paste_shortcut(&ev) {
                *self.paste_guard_until.write() = Instant::now() + PASTE_GUARD;
            }
            if ev.modifiers.is_command() {
                // A shortcut needs its modifiers held to mean anything
                // and the emitter only speaks Shift, so no faithful
                // re-emit is possible.
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
                    // A pointer press has no keyboard form to re-emit;
                    // everything else does.
                    if ev.scancode != poltertype_types::SC_POINTER_BUTTON {
                        out.stopper = Some(ev);
                    }
                    break;
                }
            }
        }
        out
    }

    /// The manual switch-last hotkey, in both of its situations.
    ///
    /// **The engine left the word alone**: switch it, bypassing every
    /// pre-decision filter, because the user asking outranks our
    /// guesses.
    ///
    /// **The engine already switched it**: put it back. Re-applying the
    /// same correction made the one gesture a user reaches for when a
    /// correction is wrong do visibly nothing. Undoing also teaches —
    /// see [`Self::learn_undone_word`].
    pub(super) fn force_switch_last(
        &self,
        last: LastWord,
        buffer: &mut WordBuffer,
        key_rx: &Receiver<KeyEvent>,
    ) {
        // Where the word is now, and where this hotkey takes it.
        let (from, target) = match last.corrected_to.clone() {
            Some(applied) => (applied, last.layout.clone()),
            None => {
                // Most plausible alternate layout. With two layouts
                // "the other one" is fine; generalising means re-running
                // the detector pipeline with `min_advantage = 0`.
                let Some(other) = self.layouts.ids().find(|id| **id != last.layout).cloned() else {
                    warn!("only one layout known; can't force-switch");
                    return;
                };
                (last.layout.clone(), other)
            }
        };
        let undoing = last.corrected_to.is_some();
        let target_mapping = match self.layouts.get(&target) {
            Some(m) => m,
            None => {
                warn!(%target, "target layout not in DB");
                return;
            }
        };
        // What is on screen right now: the user's own rendering,
        // unless our correction replaced it with the `from` one.
        let on_screen = if undoing {
            self.layouts
                .get(&from)
                .map(|m| m.translate_buffer(&last.keys))
                .unwrap_or_else(|| last.rendered.clone())
        } else {
            last.rendered.clone()
        };
        let restored = target_mapping.translate_buffer(&last.keys);
        let mut corrected = restored.clone();
        corrected.push(last.boundary_char);
        // Replay the boundary the user typed — except Enter/Tab, where
        // a re-press would submit the line or move focus. Which *key*
        // that is depends on the target layout: see `boundary_key_for`.
        let (boundary_sc, boundary_shift) = match last.boundary_scancode {
            0x1C | 0x0F | 0x60 => (0x39, false),
            sc => boundary_key_for(
                &self.layouts,
                &target,
                sc,
                last.boundary_shift,
                last.boundary_char,
            ),
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
        let applied = self.apply_correction(
            &Correction {
                from: &from,
                to: &target,
                original: &on_screen,
                corrected: &corrected,
                // The word, plus the boundary key that closed it.
                backspaces: last.keys.len() + 1,
                reason: if undoing {
                    "manual switch-last hotkey (undoing a correction)"
                } else {
                    "manual switch-last hotkey"
                },
                play_sound: true,
                replay_keys: Some(&replay),
                pointer_click_allowance: 0,
            },
            Some((key_rx, buffer)),
        );
        if applied && undoing {
            self.learn_undone_word(&target, &restored);
        }
    }

    /// Remember a word the user just rescued from a correction.
    ///
    /// Without this the auto-correction path has no escape hatch at
    /// all: "Add to dictionary" lives on the suggestion tooltip, and
    /// the tooltip only appears for words the engine *kept*.
    ///
    /// Short tokens are skipped — below three letters the dictionary
    /// runs on the curated short-stop lists rather than the FST, and a
    /// stray entry there disables correction for a whole class of real
    /// words.
    fn learn_undone_word(&self, layout: &LayoutId, word: &str) {
        let letters = poltertype_detect::letters_only_lower(word);
        if letters.chars().count() < MIN_LEARNED_LETTERS {
            debug!(
                letters = letters.chars().count(),
                "undone word is too short to learn — leaving the dictionary alone"
            );
            return;
        }
        debug!(
            %layout,
            word = %logsafe::redact_word(word),
            "learning a word from an undone correction"
        );
        let _ = self.out_tx.send(SwitcherEvent::AddToDictionary {
            layout: layout.clone(),
            word: word.to_owned(),
            origin: DictionaryAddOrigin::UndoneCorrection,
        });
    }
}
