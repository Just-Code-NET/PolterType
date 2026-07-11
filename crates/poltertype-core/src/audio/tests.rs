use super::*;

/// Both the lead and tail of the rendered buffer must be exactly
/// silent — that's what gives the audio stack room to ramp up and
/// down without clipping the user's intended fade-in / fade-out.
#[test]
fn synthesised_blip_starts_and_ends_silent() {
    let buf = synthesise_blip(440.0, 140, 1.0, 44_100);
    let samples: Vec<f32> = buf.collect();
    assert!(!samples.is_empty());
    // First sample is the start of the LEAD silence — pure 0.0.
    assert_eq!(*samples.first().unwrap(), 0.0);
    // Last sample is the end of the TAIL silence — pure 0.0.
    assert_eq!(*samples.last().unwrap(), 0.0);
}

/// At least `LEAD_SILENCE_MS` of dead air at the start. Cheap
/// regression guard: if a future refactor accidentally inlines
/// the buffer build and skips the prefix, we want to catch it.
#[test]
fn synthesised_blip_has_lead_and_tail_silence() {
    let sr = 44_100u32;
    let buf = synthesise_blip(440.0, 140, 1.0, sr);
    let samples: Vec<f32> = buf.collect();

    let lead = (u64::from(sr) * LEAD_SILENCE_MS / 1000) as usize;
    let tail = (u64::from(sr) * TAIL_SILENCE_MS / 1000) as usize;

    // Every sample in the lead window is exactly 0.0.
    assert!(
        samples[..lead].iter().all(|s| *s == 0.0),
        "lead silence window contains non-zero samples"
    );
    // Same for the tail window.
    assert!(
        samples[samples.len() - tail..].iter().all(|s| *s == 0.0),
        "tail silence window contains non-zero samples"
    );
    // And the body has some audible content.
    let body = &samples[lead..samples.len() - tail];
    assert!(!body.is_empty());
    assert!(body.iter().any(|s| s.abs() > 0.1));
}

/// The body should reach near-peak amplitude — locks in that
/// the envelope doesn't accidentally mute the whole signal (the
/// rodio `fade_out` bug we worked around did exactly that).
#[test]
fn synthesised_blip_reaches_full_amplitude() {
    let buf = synthesise_blip(440.0, 140, 1.0, 44_100);
    let peak = buf.map(f32::abs).fold(0.0_f32, f32::max);
    assert!(peak > 0.95, "expected peak ≈ 1.0, got {peak}");
}

/// Even a 30-ms event (shorter than fade_in + fade_out together
/// in the body) must still produce non-empty audio with capped
/// ramps and the surrounding silence cushion intact.
#[test]
fn synthesised_blip_handles_very_short_durations() {
    let sr = 44_100u32;
    let buf = synthesise_blip(440.0, 30, 1.0, sr);
    let samples: Vec<f32> = buf.collect();

    let lead = (u64::from(sr) * LEAD_SILENCE_MS / 1000) as usize;
    let tail = (u64::from(sr) * TAIL_SILENCE_MS / 1000) as usize;
    let tone = (u64::from(sr) * 30 / 1000) as usize;

    assert_eq!(samples.len(), lead + tone + tail);
    assert_eq!(*samples.first().unwrap(), 0.0);
    assert_eq!(*samples.last().unwrap(), 0.0);
}

/// The total buffer length matches the sum of lead + body + tail
/// at the typical 44.1 kHz sample rate. Belt-and-braces for the
/// expected event durations — a refactor that, say, accidentally
/// halved the lead silence would trip here without us having to
/// audit by ear.
#[test]
fn synthesised_blip_length_matches_padding_plan() {
    let sr = 44_100u32;
    for ms in [90u64, 140, 200] {
        let buf = synthesise_blip(440.0, ms, 1.0, sr);
        let samples: Vec<f32> = buf.collect();
        let expected = (u64::from(sr) * (LEAD_SILENCE_MS + ms + TAIL_SILENCE_MS) / 1000) as usize;
        assert_eq!(samples.len(), expected, "ms = {ms}");
    }
}
