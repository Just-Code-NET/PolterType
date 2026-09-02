//! Emitting a correction: pre-flight checks, absorbing keystrokes the
//! user lands mid-correction, the delete + replay sequence, and the
//! manual force-switch-last path.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;
use poltertype_input::{Clipboard, InputError, KeyDirection, KeyEvent, ReplayKey};
use poltertype_layout::LayoutId;
use poltertype_types::logsafe;
use tracing::{debug, info, warn};

use crate::audio::SoundEvent;
use crate::engine::buffer::{KeyKind, WordBuffer, classify};
use crate::engine::consts::{
    CHORD_RELEASE_SETTLE, CHORD_RELEASE_WAIT, CHORD_SETTLE, COPY_CHORD, HELD_FLUSH,
    HELD_FLUSH_QUIET_PROBES, INTRUSION_PROBES, INTRUSION_QUIET_PROBES, INTRUSION_REPAIRS,
    LAYOUT_SETTLE, PASTE_CHORD, PASTE_GUARD, PASTE_SETTLE, POST_EMIT_LAG, SC_BACKSPACE, SC_SPACE,
    SELECTION_COPY_WAIT, SWITCH_HOLD_PROBES, SWITCH_HOLD_STEP,
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
        let caps_on = self.caps_on();
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
            // Before the overlay because no overlay has space, and it is
            // the likeliest key to be held — the boundary that triggers
            // most corrections. Enter and Tab are deliberately absent:
            // replaying them submits a line or moves focus.
            let c = if k.scancode == SC_SPACE {
                Some(' ')
            } else {
                mapping.and_then(|m| {
                    m.translate_key(poltertype_types::WordKey {
                        scancode: k.scancode,
                        shift: k.shift,
                        // `send_text` types the codepoint itself, so
                        // the lock has to be folded in here — nothing
                        // downstream will apply it.
                        caps: caps_on,
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
            // Counts only, never the characters.
            debug!(
                dropped,
                "held keys that are neither text nor Backspace could not be replayed"
            );
        }
        Ok(())
    }

    /// Did the switch not merely happen, but **hold**?
    ///
    /// One reading is not enough. MATE's settings daemon lets the group
    /// lock land and puts its own back a moment later — measured
    /// 2026-08-24: the check 30 ms after the lock said yes, and the
    /// keystrokes emitted 60 ms after that came out in the old layout
    /// anyway. So the answer is sampled across the window the deletion
    /// would otherwise occupy, and any single "no" is a no.
    ///
    /// `None` from the backend — it cannot see past its own write —
    /// counts as held, which leaves those backends exactly as they were.
    fn switch_held(&self, to: &LayoutId) -> bool {
        for probe in 0..SWITCH_HOLD_PROBES {
            if self.layout_switcher.verify_switched(to) == Some(false) {
                return false;
            }
            if probe + 1 < SWITCH_HOLD_PROBES {
                std::thread::sleep(SWITCH_HOLD_STEP);
            }
        }
        true
    }

    /// Last resort for a desktop that ignores every way of setting the
    /// layout but its own shortcut: press that shortcut until the
    /// layout is the one we want.
    ///
    /// Returns whether `to` was reached. Nothing has been typed at this
    /// point, so giving up here costs the user nothing — which is the
    /// whole reason it sits before the deletion.
    ///
    /// The shortcut *cycles*, so this presses and checks rather than
    /// computing an index: on GNOME 49 the settings key that would name
    /// an index is inert, and the shell publishes no other. One press
    /// per layout is the bound — beyond that the desktop is not
    /// listening either.
    fn switch_by_chord(&self, to: &LayoutId) -> bool {
        let Some(chord) = self.layout_switcher.switch_chord() else {
            return false;
        };
        let steps = self
            .layout_switcher
            .list_active()
            .map(|l| l.len())
            .unwrap_or(2);
        debug!(?chord, steps, target = %to, "switching the way this desktop switches itself");
        for _ in 0..steps {
            if let Err(e) = self.key_emitter.send_chord(chord) {
                debug!(?e, "this emitter cannot send a chord");
                return false;
            }
            self.push_echoes(self.key_emitter.take_emitted());
            // The desktop's handler runs on its own event loop: the
            // shortcut is not applied by the time the last key edge is
            // written.
            std::thread::sleep(CHORD_SETTLE);
            if self.layout_switcher.verify_switched(to) == Some(true) {
                debug!(target = %to, "the desktop's own shortcut moved the layout");
                return true;
            }
        }
        false
    }

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
            // The word is redacted, so a wrong-case report has nothing
            // else to go on: this is the one bit that says whether the
            // lock was in play.
            caps = self.caps_on(),
            "applying correction"
        );

        // A same-layout replacement (spelling suggestion) has no layout
        // to flip; everything switch-related below is keyed off this.
        let switching = from != to;

        // See `LAYOUT_SETTLE`: the replay must not outrun the
        // compositor's xkb propagation.
        let mut switched_at: Option<Instant> = None;

        let mut live = live;
        let mut click_allowance = pointer_click_allowance;
        let mut tail: Vec<KeyEvent> = Vec::new();
        let mut resume: Option<KeyEvent> = None;
        let mut suspicious = false;

        // ── Wait for the user's finger to come off the hotkey ───────
        //
        // Before the layout switch, not after: neither desktop will
        // deliver a keystroke of ours anywhere useful while that key is
        // down (see `CHORD_RELEASE_WAIT`), and a correction that types
        // control characters into the user's document is worse than one
        // that does not happen. So nothing at all happens until the key
        // is up — and if it never is, the word is left as typed.
        if self.trigger_key_down() {
            if let Some((rx, _)) = live.as_ref() {
                let deadline = Instant::now() + CHORD_RELEASE_WAIT;
                while self.trigger_key_down()
                    && !suspicious
                    && resume.is_none()
                    && Instant::now() < deadline
                {
                    let w = self.drain_correction_window(rx, &mut click_allowance);
                    tail.extend(w.word_keys);
                    suspicious |= w.suspicious;
                    resume = w.resume;
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
            // Checked again rather than only on the deadline: the loop
            // also ends when something arrives that we cannot place,
            // and proceeding then would emit under the held key after
            // all.
            if self.trigger_key_down() {
                debug!(
                    "the key that asked for this correction is still down; \
                     leaving the word as it was typed"
                );
                // Whatever we swallowed while waiting is on screen and
                // no longer in the buffer's model of it.
                if (!tail.is_empty() || suspicious || resume.is_some())
                    && let Some((_, buffer)) = live.as_mut()
                {
                    self.seed_buffer(&tail, buffer);
                    buffer.poison();
                }
                return false;
            }
        }

        // Pre-flight BEFORE touching the user's text: `decide()` filters
        // candidates but `force_switch_last` bypasses that filter, and
        // settings or the OS layout list can change in between. On query
        // failure fall through and let `switch_to` surface the error —
        // still safe, nothing sent yet.
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
        // with it at the compositor, and no after-the-fact counting can
        // fix that. So fold arriving presses into the plan until the
        // stream comes back empty; a boundary means the user finished
        // their next word too, a submission or anything murkier aborts.
        if let Some((rx, _)) = live.as_ref() {
            let deadline = Instant::now() + Duration::from_millis(600);
            let mut quiet_probes = 0u8;
            loop {
                let w = self.drain_correction_window(rx, &mut click_allowance);
                tail.extend(w.word_keys);
                suspicious |= w.suspicious;
                if let Some(r) = w.resume {
                    if is_submission_scancode(r.scancode) || resume.is_some() {
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
                    // Three empty probes, two 30 ms sleeps: ~60 ms, past
                    // a fast typist's inter-key gap. Also waits for the
                    // triggering chord to come up — releasing on our
                    // side is not enough where a remapper keeps its own
                    // idea of what is down.
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
            // The switch reported success — but on a desktop whose
            // settings daemon owns the group, it can be put back before
            // a single key goes out. Going ahead then is worse than
            // doing nothing: the word is deleted and retyped
            // identically, so the user loses it and gets it back
            // unchanged. Backends that can only read their own write
            // answer `None` and this is skipped.
            if !self.switch_held(to) && !self.switch_by_chord(to) {
                warn!(
                    target = %to,
                    backend = self.layout_switcher.backend_name(),
                    "the desktop put the layout back before we could type; \
                     leaving the word alone"
                );
                return false;
            }
        }

        // ── Emit: delete → replay ───────────────────────────────────
        //
        // The gate holds the user's keys back for the length of the
        // burst; where it cannot run we probe for an intrusion
        // afterwards instead. Release whatever the user is holding
        // first — a replay under a held Ctrl produces shortcuts, not
        // text, and the correction appears not to happen at all.
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
            // apps. Unicode-emit backends answer `Unsupported` and fall
            // back to `send_text`.
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
                            // Same as `emit_held_keys`: this branch
                            // types codepoints, so the lock is ours
                            // to apply.
                            caps: self.caps_on(),
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
                    // Backspace / arrows / Esc were swallowed too — type
                    // them out after our text, where they would have
                    // landed. A shortcut needs modifiers we cannot
                    // reproduce and arrives as `None`; there all we can
                    // do is stop holding at once.
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
                // the application by itself. Hence one last sweep.
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
            // The repair is itself a burst, so it waits for a pause; if
            // none comes, the screen is left as it is.
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
            // Record where we took the word, so the manual hotkey undoes
            // this correction rather than re-applying it. Here rather
            // than in `decide`: only now is it actually on screen.
            if let Some(last) = self.last_word.write().as_mut() {
                last.corrected_to = Some(to.clone());
            }
        }

        // ── Settle & seed ───────────────────────────────────────────
        if let Some((rx, buffer)) = live {
            // Drain our own echoes before the run loop resumes:
            // `consume_echo` matches by scancode, so while the queue is
            // non-empty a real press of a scancode we just replayed
            // would be swallowed. Bounded, because backends that tag
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
                // Chronological re-assembly of what the user typed while
                // we were busy. The boundary goes through the normal
                // pipeline, so that word gets its own decision.
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

    /// Feed absorbed keystrokes into the buffer as the in-progress word:
    /// they are on screen, after the corrected boundary.
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
                // Before the release filter below: releases are the only
                // sign the triggering chord has been let go of. See
                // `modifiers_held`.
                *self.held_modifiers.write() = ev.modifiers;
                self.track_trigger_key(&ev);
                self.observe_swallowed_release(&ev);
            }
            if ev.injected || ev.direction != KeyDirection::Press {
                continue;
            }
            if self.is_own_hotkey_press(&ev) {
                // The chord that asked for this correction, held down
                // past the kernel's repeat delay. Not the user typing.
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
                // A shortcut means nothing without its modifiers held
                // and the emitter only speaks Shift, so it cannot be
                // re-emitted faithfully.
                out.suspicious = true;
                break;
            }
            let letter = self
                .layouts
                .is_letter_in_any_layout(ev.scancode, ev.modifiers.shift);
            let produced = if letter {
                None
            } else {
                self.translate_via_current_layout(
                    ev.scancode,
                    ev.modifiers.shift,
                    ev.modifiers.caps,
                )
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

    /// Convert whatever the user has *selected*, when the hotkey found
    /// no word to act on (issue #32).
    ///
    /// Only reachable with `[selection] enabled = true`, which is off
    /// by default: this is the one path that reaches into another
    /// application's clipboard, and nobody should acquire that by
    /// upgrading.
    ///
    /// The shape is forced by what a selection is. We cannot ask
    /// whether one exists — no cross-platform API answers that — so we
    /// ask the application, by copying and seeing whether anything
    /// arrives. That is also why this runs *only* when there is no word
    /// to switch: pressing `Ctrl+C` into someone's editor on every
    /// force-switch, on the chance that they had selected something,
    /// would be a cost paid by everybody for a case that is rare.
    ///
    /// Returns whether it converted anything. `false` means the caller
    /// should behave exactly as it did before this feature existed.
    pub(super) fn convert_selection(&self, key_rx: &Receiver<KeyEvent>) -> bool {
        let Some(clipboard) = self.clipboard.as_ref() else {
            debug!("selection conversion: no windowless clipboard on this session");
            return false;
        };
        if !self.wait_for_trigger_release(key_rx) {
            debug!(
                "selection conversion: the key that asked for it is still down; \
                 leaving the selection alone"
            );
            return false;
        }
        // What the clipboard held before we asked for a copy. `None` is
        // an ordinary answer — an empty clipboard, or one holding an
        // image — and *not* a reason to stop: refusing on `None` meant
        // refusing on a fresh session, which is most of them.
        //
        // Nothing is written here. The old shape staged a sentinel so
        // "unchanged" could be told from "nothing copied", and writing
        // into someone's clipboard before we know we can put it back is
        // the one thing this feature must not do. Comparing against
        // what was already there costs one rare miss instead — a
        // selection identical to the current clipboard reads as "the
        // copy never happened" — and that miss is harmless.
        let previous = clipboard.text().unwrap_or_else(|e| {
            debug!(
                ?e,
                "selection conversion: clipboard unreadable before the copy"
            );
            None
        });

        let copied = self.copy_selection(clipboard, previous.as_deref());
        let outcome = copied.as_deref().and_then(|text| self.converted(text));

        let Some((converted, from, to)) = outcome else {
            self.restore_clipboard(clipboard, previous.as_deref());
            return false;
        };
        let original = copied.unwrap_or_default();

        // Pasted, not typed. Both of the other ways are wrong for a
        // *selection*, and both were measured wrong:
        //
        // * `send_text` on Wayland goes through a Unicode-compose
        //   sequence most applications swallow or type literally —
        //   converting `ghbdsn cdsn` that way put `43f` on screen.
        // * Replaying scancodes, which is right for a single word,
        //   cannot express a selection: `Ctrl+A` in an editor takes the
        //   trailing newline with it, and no key produces one that is
        //   safe to press — Enter submits forms and chat boxes.
        //
        // A paste carries whatever the text actually is.
        if let Err(e) = clipboard.set_text(&converted) {
            warn!(
                ?e,
                "selection conversion: could not stage the converted text"
            );
            self.restore_clipboard(clipboard, previous.as_deref());
            return false;
        }
        if let Err(e) = self.key_emitter.send_chord(PASTE_CHORD) {
            debug!(?e, "selection conversion: could not send the paste chord");
            self.restore_clipboard(clipboard, previous.as_deref());
            return false;
        }
        self.push_echoes(self.key_emitter.take_emitted());
        // The application reads the clipboard when it handles the
        // paste, not when the keys arrive, so the restore has to wait
        // for it. Too short and the user gets their old clipboard
        // pasted; there is no handshake to wait on instead.
        std::thread::sleep(PASTE_SETTLE);
        self.restore_clipboard(clipboard, previous.as_deref());

        info!(%from, %to, "selection converted");
        let _ = self.out_tx.send(SwitcherEvent::Corrected {
            from_layout: from,
            to_layout: to,
            original_text: original,
            corrected_text: converted,
            reason: "selection conversion".into(),
        });
        true
    }
}

impl SwitcherEngine {
    /// Wait for the finger to come off the hotkey before anything is
    /// emitted — the same fact `apply_correction` waits on, on the one
    /// force-switch path that was not waiting on it.
    ///
    /// While a grabbed chord's key is down, X11's passive grab is an
    /// *active* one, and everything we emit goes to the client holding
    /// it rather than to the focused window. A `Ctrl+C` sent then never
    /// reaches the application the selection is in, the clipboard never
    /// changes, and the gesture reads as "nothing was selected" —
    /// which is how issue #51 read on Cinnamon X11. Releasing the
    /// modifiers we can see does not help: the grab is held by the
    /// chord's own key, and that is still physically down.
    ///
    /// Waiting is possible here for the same reason it is there: the
    /// wait runs on the thread that reads key events, so it has to
    /// drain them itself, and `drain_correction_window` is what keeps
    /// `trigger_key_down` answering honestly while it does.
    ///
    /// `false` — do nothing at all — when the key never comes up, or
    /// when the user typed while we waited: what they typed replaced
    /// the selection, so there is nothing left to convert.
    fn wait_for_trigger_release(&self, key_rx: &Receiver<KeyEvent>) -> bool {
        if !self.trigger_key_down() {
            return true;
        }
        let deadline = Instant::now() + CHORD_RELEASE_WAIT;
        let mut click_allowance = 0;
        while self.trigger_key_down() && Instant::now() < deadline {
            if self
                .drain_correction_window(key_rx, &mut click_allowance)
                .saw_user_press
            {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        !self.trigger_key_down()
    }

    /// Press the platform's copy chord and wait for the clipboard to
    /// stop reading as `previous`.
    ///
    /// Polled rather than slept on: the clipboard is not readable the
    /// instant the chord goes out — the application has to notice, and
    /// on Wayland ownership changes hands asynchronously — but waiting
    /// a fixed worst case would make every miss feel like a hang.
    fn copy_selection(
        &self,
        clipboard: &Arc<dyn Clipboard>,
        previous: Option<&str>,
    ) -> Option<String> {
        // The hotkey that got us here fires on the *press*, so its own
        // `Ctrl+Shift` is still down, and `Ctrl+C` under a held Shift
        // is `Ctrl+Shift+C` — not copy in any application. The
        // clipboard then never changes and the whole thing reads as
        // "nothing was selected", which is exactly how it read on KDE
        // before this.
        //
        // Released rather than waited for. Waiting cannot work here:
        // this runs on the thread that reads key events, so the release
        // that would end the wait cannot arrive until we return. Same
        // call, same reason, as the replay path a few hundred lines
        // down — and the modifiers are deliberately not pressed back,
        // since re-pressing one the user has let go of leaves it stuck.
        let holding = *self.held_modifiers.read();
        if holding.control || holding.shift || holding.alt || holding.meta {
            if let Err(e) = self.key_emitter.release_modifiers(holding) {
                debug!(?e, "selection conversion: could not release the held chord");
                return None;
            }
            self.push_echoes(self.key_emitter.take_emitted());
            // The compositor needs the releases before the chord that
            // follows them, or it reads both in the same frame.
            std::thread::sleep(CHORD_RELEASE_SETTLE);
        }
        if let Err(e) = self.key_emitter.send_chord(COPY_CHORD) {
            debug!(
                ?e,
                "selection conversion: this backend cannot send a copy chord"
            );
            return None;
        }
        self.push_echoes(self.key_emitter.take_emitted());
        let deadline = Instant::now() + SELECTION_COPY_WAIT;
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
            match clipboard.text() {
                Ok(Some(text)) if !text.is_empty() && Some(text.as_str()) != previous => {
                    return Some(text);
                }
                Ok(_) => {}
                Err(e) => {
                    debug!(?e, "selection conversion: clipboard unreadable mid-copy");
                    return None;
                }
            }
        }
        debug!("selection conversion: nothing was selected");
        None
    }

    /// The selection re-rendered under another layout, or `None` when
    /// it is not wrong-layout text at all.
    fn converted(&self, text: &str) -> Option<(String, LayoutId, LayoutId)> {
        let from = self.layout_switcher.current().ok()?;
        let to = self.next_layout_after(&from)?;
        let source = self.layouts.get(&from)?;
        let target = self.layouts.get(&to)?;
        let converted = source.transliterate_to(text, target)?;
        Some((converted, from, to))
    }

    /// Put back what the user had, best effort. A failure here is worth
    /// a line: they lost a clipboard because of us.
    fn restore_clipboard(&self, clipboard: &Arc<dyn Clipboard>, previous: Option<&str>) {
        if let Some(prev) = previous
            && let Err(e) = clipboard.set_text(prev)
        {
            warn!(?e, "selection conversion: could not restore the clipboard");
        }
    }

    /// The layout a force-switch moves a word to, given the one it
    /// reads in now: the next along the OS's own list of active
    /// layouts, wrapping. With two layouts that is just "the other
    /// one"; with three it is what lets repeated presses reach the
    /// third instead of bouncing between the first two (issue #37).
    ///
    /// The order has to be the same in every process. `HashMap::keys()`
    /// was not, which is harmless while the DB holds exactly the two
    /// active layouts and not at all harmless when it does not: a
    /// failed `list_active()` loads all fifteen bundled layouts, the
    /// force-switch aims at whichever came out first, and the
    /// pre-flight refuses it — no keystroke, no word, no explanation.
    /// So: what the OS says is switchable, else the DB sorted by name.
    fn next_layout_after(&self, current: &LayoutId) -> Option<LayoutId> {
        let mut ring: Vec<LayoutId> = self
            .layout_switcher
            .list_active()
            .unwrap_or_default()
            .into_iter()
            .filter(|id| self.layouts.get(id).is_some())
            .collect();
        if ring.len() < 2 || !ring.contains(current) {
            ring = self.layouts.ids().cloned().collect();
            ring.sort();
        }
        let at = ring.iter().position(|id| id == current)?;
        let next = ring[(at + 1) % ring.len()].clone();
        (next != *current).then_some(next)
    }

    /// The manual switch-last hotkey, in both of its situations.
    ///
    /// **The engine already switched the word**: put it back, and teach
    /// the word (see [`Self::learn_undone_word`]). Re-applying the same
    /// correction would make the one gesture a user reaches for when a
    /// correction is wrong do visibly nothing.
    ///
    /// **Anything else** — a word the engine left alone, or one this
    /// same hotkey moved a moment ago — rotates on to the next layout,
    /// bypassing every pre-decision filter, because the user asking
    /// outranks our guesses. Rotating rather than undoing is what makes
    /// the gesture repeatable: press again to take back a press that
    /// was itself a mistake, or again to reach a third layout.
    /// Returns `false` when nothing happened — the trigger key never
    /// came up, the target layout went away, the desktop put the
    /// layout back. The caller is holding the only copy of the word by
    /// then, and a gesture that did nothing must not have eaten it.
    pub(super) fn force_switch_last(
        &self,
        last: LastWord,
        buffer: &mut WordBuffer,
        key_rx: &Receiver<KeyEvent>,
    ) -> bool {
        // Whatever the word reads in right now is where the switch
        // starts from, and it is the engine's correction — never our
        // own earlier press — that a press undoes.
        let from = last
            .corrected_to
            .clone()
            .unwrap_or_else(|| last.layout.clone());
        let undoing = last.corrected_to.is_some() && !last.user_placed;
        let target = if undoing {
            last.layout.clone()
        } else {
            let Some(next) = self.next_layout_after(&from) else {
                warn!("only one layout known; can't force-switch");
                return false;
            };
            next
        };
        let target_mapping = match self.layouts.get(&target) {
            Some(m) => m,
            None => {
                warn!(%target, "target layout not in DB");
                return false;
            }
        };
        // What is on screen right now: the user's own rendering, unless
        // something — our correction, or an earlier press of this very
        // hotkey — replaced it with the `from` one.
        let on_screen = if from == last.layout {
            last.rendered.clone()
        } else {
            self.layouts
                .get(&from)
                .map(|m| m.translate_buffer(&last.keys))
                .unwrap_or_else(|| last.rendered.clone())
        };
        let restored = target_mapping.translate_buffer(&last.keys);
        let mut corrected = restored.clone();
        let mut replay: Vec<ReplayKey> = last
            .keys
            .iter()
            .map(|k| ReplayKey {
                scancode: k.scancode,
                shift: k.shift,
            })
            .collect();
        // The word plus, if there is one, the boundary key that closed
        // it. A word still being typed has none: nothing follows the
        // caret to backspace over or put back.
        let mut backspaces = last.keys.len();
        if let Some(b) = last.boundary {
            corrected.push(b.ch);
            // Enter/Tab excepted, where a re-press would submit the line
            // or move focus. Which *key* carries the character depends
            // on the target layout: see `boundary_key_for`.
            let (boundary_sc, boundary_shift) = match b.scancode {
                0x1C | 0x0F | 0x60 => (0x39, false),
                sc => boundary_key_for(&self.layouts, &target, sc, b.shift, b.ch),
            };
            replay.push(ReplayKey {
                scancode: boundary_sc,
                shift: boundary_shift,
            });
            backspaces += 1;
        }
        let applied = self.apply_correction(
            &Correction {
                from: &from,
                to: &target,
                original: &on_screen,
                corrected: &corrected,
                backspaces,
                reason: if undoing {
                    "manual switch-last hotkey (undoing a correction)"
                } else {
                    "manual switch-last hotkey"
                },
                play_sound: self.settings.snapshot().general.sound_on_correct,
                replay_keys: Some(&replay),
                pointer_click_allowance: 0,
            },
            Some((key_rx, buffer)),
        );
        if !applied {
            return false;
        }
        if undoing {
            self.learn_undone_word(&target, &restored, &from, &on_screen);
        }
        // Put back what is now on screen, so the hotkey can be pressed
        // again (issue #37): getting here consumed the stash, and
        // without this the second press finds nothing and the gesture
        // works exactly once per word. `user_placed` marks the new
        // rendering as the user's own doing, which is what stops the
        // next press reading as "undo a correction" and teaching the
        // dictionary a word nobody rescued.
        *self.last_force_switch.write() = Some(Instant::now());
        // The word on screen now reads in `target`, and a word still
        // being typed is still the buffer's. Leaving the old stamp
        // here made the next press compute the rotation from the
        // layout the word *used* to be in and retype it unchanged.
        *self.word_layout.write() = Some(target.clone());
        *self.last_word.write() = Some(LastWord {
            corrected_to: (target != last.layout).then(|| target.clone()),
            keys: last.keys,
            rendered: last.rendered,
            layout: last.layout,
            boundary: last.boundary,
            user_placed: true,
        });
        true
    }

    /// Convert the separator the caret is sitting after, when there is
    /// no word to convert (issue #52).
    ///
    /// `№` is `Shift+3` on the Russian and Ukrainian layouts and `#` on
    /// the US one. It is not a letter, so it never joins a word, never
    /// reaches the stash, and the manual hotkey had nothing to act on —
    /// while the key that produced it is perfectly well known. One
    /// character only: what the report asked for is the separator
    /// immediately left of the caret, and a run of them is as likely to
    /// be a divider line as a mistake.
    ///
    /// Returns `false` — leaving the text alone — whenever the switch
    /// would be pointless or destructive rather than wrong: a key that
    /// reads the same under both layouts (every space is a space), and
    /// the submission keys, whose replay would send the line or move
    /// focus instead of typing a character.
    pub(super) fn force_switch_separator(
        &self,
        buffer: &mut WordBuffer,
        key_rx: &Receiver<KeyEvent>,
    ) -> bool {
        let Some(&(scancode, shift)) = buffer.boundary_run().last() else {
            return false;
        };
        if is_submission_scancode(scancode) || scancode == SC_SPACE {
            return false;
        }
        let Ok(from) = self.layout_switcher.current() else {
            return false;
        };
        let Some(to) = self.next_layout_after(&from) else {
            debug!("only one layout known; can't switch a separator");
            return false;
        };
        let render = |id: &LayoutId| {
            self.layouts.get(id).and_then(|m| {
                m.translate_key(poltertype_types::WordKey {
                    scancode,
                    shift,
                    caps: false,
                    timestamp_ms: 0,
                })
            })
        };
        let (Some(original), Some(corrected)) = (render(&from), render(&to)) else {
            return false;
        };
        if original == corrected {
            debug!(%from, %to, "the separator under the caret reads the same in both layouts");
            return false;
        }
        self.apply_correction(
            &Correction {
                from: &from,
                to: &to,
                original: &original.to_string(),
                corrected: &corrected.to_string(),
                backspaces: 1,
                reason: "manual switch-last hotkey (separator)",
                play_sound: self.settings.snapshot().general.sound_on_correct,
                // The same physical key, retyped under the layout we
                // have just switched to — which is the whole of what
                // turns `№` into `#`.
                replay_keys: Some(&[ReplayKey { scancode, shift }]),
                pointer_click_allowance: 0,
            },
            Some((key_rx, buffer)),
        )
    }

    /// Remember a word the user just rescued from a correction — the
    /// auto-correction path's only escape hatch, since "Add to
    /// dictionary" lives on a tooltip that appears only for words the
    /// engine *kept*.
    ///
    /// Short tokens are skipped: below three letters the dictionary runs
    /// on the curated short-stop lists rather than the FST, and a stray
    /// entry there disables correction for a whole class of real words.
    fn learn_undone_word(&self, target: &LayoutId, restored: &str, from: &LayoutId, undone: &str) {
        let letters = poltertype_detect::letters_only_lower(restored);
        if letters.chars().count() < MIN_LEARNED_LETTERS {
            debug!(
                letters = letters.chars().count(),
                "undone word is too short to learn — leaving the dictionary alone"
            );
            return;
        }
        if !self.undo_vouches_for_word(target, restored, from, undone) {
            debug!(
                %target,
                %from,
                word = %logsafe::redact_word(restored),
                "undo of a dictionary-backed correction — not learning it as a word"
            );
            return;
        }
        debug!(
            %target,
            word = %logsafe::redact_word(restored),
            "learning a word from an undone correction"
        );
        let _ = self.out_tx.send(SwitcherEvent::AddToDictionary {
            layout: target.clone(),
            word: restored.to_owned(),
            origin: DictionaryAddOrigin::UndoneCorrection,
        });
    }

    /// Does this undo claim "the restored text is a word of `target`",
    /// or only "not this one, not now"?
    ///
    /// A claim, when `target` already knows the word — an overlay entry
    /// then promotes it past a `weak` flag or a short-list gap — or when
    /// `from` does *not* know what was on screen, meaning the engine
    /// switched on word shape rather than on dictionary evidence.
    ///
    /// Otherwise the correction rested on a real word of the other
    /// language and the restored text is that word's wrong-layout twin.
    /// Learning it writes gibberish into the dictionary for good:
    /// `ghbdsn` (uk-UA `привіт` under en-US) and `ефілі` (en-US `tasks`
    /// under uk-UA) both arrived that way, from undos of corrections the
    /// engine had got right, and each then broke the real word.
    ///
    /// No suggestion provider means no dictionary to ask, which fails
    /// open; `DictionaryDetector::judge` stops a stray entry outranking
    /// a real word either way.
    fn undo_vouches_for_word(
        &self,
        target: &LayoutId,
        restored: &str,
        from: &LayoutId,
        undone: &str,
    ) -> bool {
        let Some(provider) = self.suggester.as_ref() else {
            return true;
        };
        provider.is_known(target, restored) || !provider.is_known(from, undone)
    }
}
