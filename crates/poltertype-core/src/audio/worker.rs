//! The audio worker thread: playback and tone synthesis.

use super::*;
use rodio::buffer::SamplesBuffer;
use rodio::{Decoder, OutputStreamHandle, Sink};
use std::fs::File;
use std::io::BufReader;
use tracing::{debug, info, warn};

pub(crate) fn run_worker(rx: crossbeam_channel::Receiver<AudioCmd>) {
    use crossbeam_channel::RecvTimeoutError;

    info!("audio worker started (cached OutputStream + idle release)");

    let mut state = WorkerState::new();

    loop {
        // Block indefinitely while there is no stream to release;
        // only poll on a timeout while a stream is cached. Waiting
        // with `recv_timeout` unconditionally would wake the worker
        // every 30 s for the life of the process just to discover
        // there is nothing to drop.
        let cmd = if state.stream.is_some() {
            match rx.recv_timeout(STREAM_IDLE_REFRESH) {
                Ok(cmd) => cmd,
                Err(RecvTimeoutError::Timeout) => {
                    // No plays for a whole refresh window — release
                    // the stream. A long-lived open CoreAudio output
                    // on an HDMI / DisplayPort device keeps
                    // coreaudiod's power assertion alive, which on
                    // macOS blocks display sleep and system sleep.
                    // Dropping the stream hands the device back and
                    // costs only a ~20-50 ms reopen on the next sound
                    // (cushioned by LEAD_SILENCE_MS).
                    debug!("audio: idle timeout — releasing output stream");
                    state.invalidate();
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match rx.recv() {
                Ok(cmd) => cmd,
                Err(_) => break,
            }
        };
        match cmd {
            AudioCmd::Refresh {
                theme_dir: d,
                volume: v,
            } => {
                state.theme_dir = d;
                state.volume = v;
                debug!(theme_dir = ?state.theme_dir, volume = state.volume, "audio refreshed");
            }
            AudioCmd::Play(event) => {
                play_event(&mut state, event);
            }
            AudioCmd::Shutdown => break,
        }
    }
    info!("audio worker stopped");
}

/// Resolve theme-vs-synth, then play. On failure, invalidate the
/// cached stream and retry exactly once — that's enough to recover
/// from the common "default device just changed" case without
/// turning every play into a flaky retry loop.
pub(crate) fn play_event(state: &mut WorkerState, event: SoundEvent) {
    for attempt in 0..2 {
        let Some(handle) = state.handle() else {
            // No device available; no point retrying inside this
            // event. Next event will try again.
            return;
        };
        let result = play_with_handle(&handle, event, state.theme_dir.as_deref(), state.volume);
        match result {
            Ok(()) => return,
            Err(e) => {
                warn!(?e, attempt, event = ?event, "audio play failed");
                // Drop the cached stream — likely stale (default
                // device changed mid-play, USB unplugged, …).
                state.invalidate();
            }
        }
    }
}

/// One shot: play either the user's theme file or the synthesised
/// fallback. The handle is borrowed from the cached stream — caller
/// owns the lifetime.
pub(crate) fn play_with_handle(
    handle: &OutputStreamHandle,
    event: SoundEvent,
    theme_dir: Option<&std::path::Path>,
    volume: f32,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if let Some(dir) = theme_dir {
        let path = dir.join(event.file_name());
        if path.exists() {
            return play_file(handle, &path, volume);
        }
    }
    play_tone(handle, event, volume)
}

pub(crate) fn play_file(
    handle: &OutputStreamHandle,
    path: &std::path::Path,
    volume: f32,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let file = BufReader::new(File::open(path)?);
    let decoder = Decoder::new(file)?;
    let sink = Sink::try_new(handle)?;
    sink.set_volume(volume);
    sink.append(decoder);
    // Block the worker thread until the sink drains. The cached
    // OutputStream stays alive across calls, so the OS audio buffer
    // is never torn down mid-tail.
    sink.sleep_until_end();
    Ok(())
}

/// Play a synthesised fallback tone for `event`. Pre-rendered as a
/// `SamplesBuffer` with silence padding around a fade-in/fade-out
/// envelope so the audible part is never clipped by stream init or
/// buffer-drain timing.
pub(crate) fn play_tone(
    handle: &OutputStreamHandle,
    event: SoundEvent,
    volume: f32,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (freq, ms) = event.synth_tone();
    let amp = (volume * 0.4).clamp(0.0, 1.0);
    let source = synthesise_blip(freq, ms, amp, /* sample_rate */ 44_100);
    let sink = Sink::try_new(handle)?;
    sink.append(source);
    sink.sleep_until_end();
    Ok(())
}

/// Build a single-channel `SamplesBuffer` for the given tone:
///
/// ```text
///   [LEAD silence] [fade-in] [body] [fade-out] [TAIL silence]
///   |----30ms----| |--10ms--|       |--25ms--| |----60ms----|
/// ```
///
/// Ramps and padding do different jobs. The **ramps** avoid the
/// sample-level discontinuity of cutting a sine mid-cycle, which a
/// speaker reproduces as a click. **Lead silence** absorbs OS audio
/// warmup on a freshly opened stream, without which the fade-in is
/// partly eaten by device init. **Tail silence** lets the OS buffer
/// flush the last real samples before the sink drops.
///
/// The envelope is computed by hand rather than through rodio's
/// `fade_in` / `fade_out` because 0.20's `fade_out` starts ramping at
/// sample 0 and reaches silence at `duration` — the opposite of what
/// the name suggests.
pub(crate) fn synthesise_blip(
    freq: f32,
    ms: u64,
    amp: f32,
    sample_rate: u32,
) -> SamplesBuffer<f32> {
    let sr = u64::from(sample_rate);
    let lead_silence_n = (sr * LEAD_SILENCE_MS / 1000) as usize;
    let tail_silence_n = (sr * TAIL_SILENCE_MS / 1000) as usize;
    let tone_total = (sr.saturating_mul(ms) / 1000) as usize;

    // Ramp lengths, capped to a third of the tone's body so even a
    // very short event still has audible sustain between ramps.
    let cap = tone_total / 3;
    let fade_in = (sr * 10 / 1000) as usize;
    let fade_out = (sr * 25 / 1000) as usize;
    let fade_in = fade_in.min(cap);
    let fade_out = fade_out.min(cap);

    let two_pi_f = 2.0 * std::f32::consts::PI * freq;
    let inv_sr = 1.0 / sample_rate as f32;

    let mut samples = Vec::with_capacity(lead_silence_n + tone_total + tail_silence_n);

    // Lead silence cushion.
    samples.resize(lead_silence_n, 0.0);

    // Tone body with linear fade-in and fade-out, anchored to exact
    // 0.0 at sample 0 and at sample (tone_total - 1).
    for i in 0..tone_total {
        let envelope = if fade_in > 1 && i < fade_in {
            i as f32 / (fade_in - 1) as f32
        } else if fade_out > 1 && i >= tone_total.saturating_sub(fade_out) {
            let from_end = tone_total - 1 - i;
            from_end as f32 / (fade_out - 1) as f32
        } else {
            1.0
        };
        let t = i as f32 * inv_sr;
        let v = (two_pi_f * t).sin() * envelope * amp;
        samples.push(v);
    }

    // Tail silence cushion.
    samples.resize(samples.len() + tail_silence_n, 0.0);

    SamplesBuffer::new(1, sample_rate, samples)
}
