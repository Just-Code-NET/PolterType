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
use crate::engine::heuristics::{is_paste_shortcut, is_submission_scancode};
use crate::engine::types::{Correction, HeldKeys, LastWord, WindowDrain};

use super::engine::SwitcherEngine;

/// Shortest word an undone correction may teach the dictionary. Same
/// three-letter floor the suggestion tooltip uses, for the same
/// reason: below it the engine is not working from the FST at all.
const MIN_LEARNED_LETTERS: usize = 3;

impl SwitcherEngine {
    /// Type out keystrokes the key gate held back, by whichever emit
    /// path this backend actually has.
    ///
    /// `send_keys` replays raw scancodes — what Wayland needs, and what
    /// the gate was written against, since it was born on evdev.
    /// Windows and macOS answer `Unsupported` on purpose: their
    /// Unicode-emit API is the more robust path there, and replaying
    /// scancodes would race the layout switch we have just asked for.
    /// The correction itself has fallen back to `send_text` for that
    /// reason since the beginning; **this path had not**, so on those
    /// platforms the gate swallowed the user's keystrokes from the
    /// application and then failed to give them back.
    ///
    /// Held-and-dropped is strictly worse than never held: it is the
    /// one way this feature can *lose* typing rather than merely
    /// scramble it, which is why the gate stays off by default until a
    /// platform is known to come back from here. Found the first time
    /// the Windows gate ran on real hardware (2026-08-04): every burst
    /// logged `no scancode-replay path` and the characters were gone.
    ///
    /// What the fallback cannot reproduce is a keystroke that is not a
    /// character — Backspace, the arrows, Esc translate to nothing in
    /// any layout and are dropped. That is the same narrow loss the
    /// mid-correction shortcut already carries, and it is bounded by
    /// the length of one burst.
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
            // Backspace is not text. It has to go out as a keypress,
            // and in its place in the sequence — the user pressed it to
            // delete what they typed just before it, so emitting it
            // after the rest would eat the wrong character.
            if k.scancode == SC_BACKSPACE {
                self.flush_text(&mut text)?;
                let sent = self.key_emitter.send_backspaces(1);
                self.push_echoes(self.key_emitter.take_emitted());
                sent?;
                continue;
            }
            // Space before the overlay, because no overlay has it: a
            // layout describes the 46 character keys, so the spacebar
            // translates to nothing and used to be dropped. It is also
            // the single most likely key to be held, being the boundary
            // that triggers most corrections.
            //
            // Enter and Tab are deliberately *not* here: replaying them
            // submits a line or moves focus, which `is_submission_scancode`
            // already refuses to do anywhere else in a correction.
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

        // A same-layout replacement (spelling suggestion) has no
        // layout to flip and no pre-flight to run — everything below
        // that is switch-related is keyed off this.
        let switching = from != to;

        // When the layout flip happened — the replay must not outrun
        // the compositor's xkb propagation. See `LAYOUT_SETTLE`.
        let mut switched_at: Option<Instant> = None;

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

            // Switch the layout FIRST. Flipping the layout doesn't touch
            // existing text, so if it fails we abort with the user's word
            // fully intact (the old order — backspaces, then switch —
            // destroyed the word whenever the switch failed). It also
            // overlaps the compositor's xkb propagation with the backspace
            // burst, and means any keystrokes the user lands mid-correction
            // already produce glyphs in the layout they intended.
            if let Err(e) = self.layout_switcher.switch_to(to) {
                warn!(?e, target = %to, "layout switch failed; aborting correction before any keystrokes");
                return false;
            }
            switched_at = Some(Instant::now());
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
        // already, in the layout we just switched to). Only once the
        // stream has come back empty three times running (~60 ms of
        // silence, past a fast typist's inter-key gap) do we start
        // emitting. The absorbed tail is deleted together with the
        // word and re-typed after the boundary, preserving order.
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
                    // Three empty probes, two 30 ms sleeps between
                    // them: ~60 ms of silence. A fast typist's
                    // inter-key gap is ~45-60 ms plus listener lag, so
                    // two probes can land inside a single gap.
                    //
                    // A correction fired by a chord waits for that
                    // chord to come up as well. Our replay reaches the
                    // application the same way the user's keys do, so
                    // typing under their held `Ctrl` produces
                    // shortcuts and nothing lands — telling the
                    // emitter to release the modifiers is not enough
                    // where a remapper keeps its own idea of what is
                    // down. The deadline below bounds the wait.
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
            // Nothing emitted yet — bail out with the text untouched.
            // The buffer can't vouch for the screen any more, though:
            // taint it and drop the manual-switch stash.
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

        // Wait out the compositor's xkb propagation before touching
        // anything — here rather than just before the replay, so it
        // can't widen the gap between our last look at the key stream
        // and our first emitted key. Normally already elapsed.
        if let Some(t) = switched_at {
            let since = t.elapsed();
            if since < LAYOUT_SETTLE {
                std::thread::sleep(LAYOUT_SETTLE - since);
            }
        }

        // ── Emit: delete → replay ───────────────────────────────────
        //
        // Erase the on-screen characters, then retype the corrected
        // word plus everything the user typed while we were preparing.
        //
        // A keystroke that lands *inside* that burst is ordered against
        // our emitted events by the compositor, and no after-the-fact
        // counting can undo it: `зтзь ш ` came out as `ipnpm ` because
        // the `i` reached the app between our deletion and our replay,
        // and `pinpm ` / `pnpmi ` when it reached it mid-replay.
        //
        // So we hold the user's keys back for exactly as long as the
        // burst takes. Held keys still reach us — they just queue up
        // instead of landing in our text, and we type them out
        // ourselves once the correction is down. Where the gate can't
        // run (no evdev, or a remapper in the way) we fall back to
        // probing for an intrusion afterwards and re-emitting.
        // Let go of anything the user is holding before typing. A
        // chord-triggered correction (suggestion accept, manual
        // switch-last) fires with its own modifiers still down, and a
        // replay under a held Ctrl produces shortcuts, not text — the
        // user sees the correction simply not happen.
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
            // A bounded compensation loop catches the stragglers that
            // still manage to land during the burst itself: each one
            // both soaked up one of our backspaces and must be deleted
            // and re-typed, so it costs exactly one extra backspace
            // either way. The loop exits on a probe that comes back
            // empty, and the replay follows immediately after it.
            for round in 0..3 {
                let sent = self.key_emitter.send_backspaces(to_delete);
                self.push_echoes(self.key_emitter.take_emitted());
                if let Err(e) = sent {
                    warn!(?e, "send_backspaces failed; aborting correction");
                    return false;
                }
                let Some((rx, _)) = live.as_ref() else { break };
                // With the keyboard held, nothing of the user's can
                // have reached the screen, so there is nothing to
                // compensate for — what they typed is waiting for us
                // and gets typed out after the replay instead.
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
            // Prefer replaying the original scancodes against the
            // freshly switched layout (the only path that works in
            // Wayland-native / terminal apps). Backends that have a
            // real Unicode-emit API (`KEYEVENTF_UNICODE`,
            // `CGEventKeyboardSetUnicodeString`) return `Unsupported`;
            // we fall back to `send_text` for them.
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
            // These keys never reached the application, so there is
            // nothing on screen to delete and nothing to disentangle —
            // they simply go on the end, in the order they were
            // pressed. Keep going while the user keeps typing, up to a
            // bound; whatever they press after we let go reaches the
            // application by itself.
            if held.active() {
                let flush_deadline = Instant::now() + HELD_FLUSH;
                // One empty sweep is not "the user stopped" — it is
                // shorter than an inter-key gap, and letting go on it
                // drops whatever they press a moment later into the
                // hole between our last sweep and the actual ungrab.
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
                    // Backspace / arrows / Esc were swallowed too. They
                    // are the user editing, so they have to be typed
                    // out — after our text, which is where they would
                    // have landed had we not been in the way. A
                    // shortcut needs modifiers we cannot reproduce and
                    // arrives here as `None`; all we can do is stop
                    // holding immediately so the next one gets through.
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
                // Letting go is synchronous, so the moment it returns
                // the line is drawn: everything already on the stream
                // was held back and is ours to type out, everything
                // after it reaches the application by itself. One last
                // sweep collects the stragglers on our side of it.
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
                    // Not `send_keys` directly: on macOS and Windows
                    // that is `Unsupported`, and these keystrokes were
                    // already swallowed from the application — dropping
                    // them here would lose them outright. Same fix as
                    // the main flush path; the second call site was
                    // missed when `emit_held_keys` got its fallback.
                    if let Err(e) = self.emit_held_keys(&last, to) {
                        warn!(?e, "flushing the last held keystrokes failed");
                    }
                }
                break;
            }

            // ── Intrusion probe (gate unavailable) ──────────────────
            //
            // Anything on the wire now was pressed while the replay was
            // going out (the deletion loop left the stream quiet
            // moments ago), so it is on screen somewhere *inside* the
            // text we just typed. We can't tell where — but we know
            // exactly how many characters we put down, so erasing that
            // many plus the intruders and retyping puts everything back
            // in typed order.
            //
            // The repair is another burst, though, and firing it while
            // the user is still mid-word just hands the next keystroke
            // the same race to win. So wait for a pause first, and if
            // one never comes, leave the screen exactly as it is and
            // stop vouching for it — a scrambled word the user can fix
            // beats a correction chasing their fingers across the line.
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
                // Spent the budget, or the user never paused. The
                // screen holds something we did not put there and
                // cannot place — track nothing, correct nothing.
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
        // `Corrected` / `LayoutChanged` are layout-correction events;
        // a same-layout replacement announces itself via
        // `SuggestionApplied` from its own caller instead.
        if switching {
            let _ = self.out_tx.send(SwitcherEvent::Corrected {
                from_layout: from.clone(),
                to_layout: to.clone(),
                original_text: original.to_owned(),
                corrected_text: corrected.to_owned(),
                reason: reason.to_owned(),
            });
            let _ = self.out_tx.send(SwitcherEvent::LayoutChanged(to.clone()));
            // The stashed word now reads differently on screen than
            // the user typed it, so record where we took it — the
            // manual hotkey undoes a correction rather than re-
            // applying one. Here rather than in `decide`, because
            // only this far in is the correction actually on screen;
            // and before the settle-and-seed below, which can run a
            // later word through the pipeline and stash its own.
            if let Some(last) = self.last_word.write().as_mut() {
                last.corrected_to = Some(to.clone());
            }
        }

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
        true
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
    /// shortcut) sets `suspicious`. `click_allowance` pointer presses
    /// are swallowed benignly — see `apply_correction`.
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
                // sign that the chord which triggered this correction
                // has been let go of — see `modifiers_held`.
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
                // A shortcut needs its modifiers held to mean anything,
                // and the emitter only speaks Shift — no faithful
                // re-emit, so it is deliberately left as `None`.
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
    /// **The engine left the word alone** — the usual case, and the
    /// one the hotkey was built for: switch it to the other layout,
    /// bypassing every pre-decision filter, because the user asking
    /// outranks our guesses.
    ///
    /// **The engine already switched the word** — the hotkey puts it
    /// back. It used to re-apply the same correction: same keys, same
    /// target layout, so the word was deleted and retyped identically
    /// and the one gesture a user reaches for when a correction is
    /// wrong did visibly nothing. Undoing also teaches: a user who
    /// takes a correction back has told us, as plainly as the tooltip
    /// button does, that this word is right as typed — see
    /// [`Self::learn_undone_word`].
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
                // Pick the most plausible alternate layout — in v0.1
                // with two layouts, "the other one" is fine.
                // Generalisation will re-run the detector pipeline
                // with `min_advantage = 0`.
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
    /// The alternative was leaving the auto-correction path with no
    /// escape hatch at all: the "Add to dictionary" row only exists
    /// on the suggestion tooltip, and the tooltip only appears for
    /// words the engine *kept*. Words it corrected — the ones that
    /// actually cost the user something when the engine is wrong —
    /// could be undone one at a time, for ever, and the app never
    /// learned a thing.
    ///
    /// Short tokens are skipped. Below three letters the dictionary
    /// runs on the curated short-stop lists rather than the FST (see
    /// `LayoutDictionary`), and a stray one- or two-letter entry
    /// there disables correction for a whole class of real words.
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
