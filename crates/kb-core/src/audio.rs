//! Sound effects via `rodio`, owned by a dedicated worker thread.
//!
//! Why a worker thread: rodio's `OutputStream` is `!Send` on most
//! platforms because the underlying audio API (CoreAudio, ALSA, …)
//! ties a stream to its creating thread. Wrapping it behind a
//! crossbeam channel keeps `AudioPlayer` `Send + Sync` so the engine
//! can hold an `Arc<AudioPlayer>` on its own thread.
//!
//! Stream lifecycle: we cache **one** `OutputStream` on the worker
//! thread and reuse it across plays. Two reasons for this design vs
//! the obvious "fresh stream per play":
//!
//!   * Per-play `OutputStream::try_default()` costs 20-50 ms on
//!     Windows / macOS and visibly eats the first few milliseconds of
//!     the synth tone — the user hears a clipped, "broken" sound
//!     instead of the intended fade-in.
//!   * The same call also fails intermittently when the OS default
//!     device is mid-switch (BT headphones connecting, HDMI cable
//!     plugged in, …) — leading to silent plays.
//!
//! Default-device tracking is preserved with a *stale refresh*: if
//! the cached stream hasn't been used for [`STREAM_IDLE_REFRESH`],
//! the next play drops it and reopens against the (possibly new)
//! default device. Plus, any play error invalidates the cached
//! stream so the next attempt starts from a clean slate. Together
//! these handle "user just plugged in headphones" gracefully without
//! paying the per-play cost during normal pause / resume bursts.
//!
//! Themes live in `<config-dir>/sound-themes/<name>/<event>.ogg`.
//! Missing files are silent — we never crash because audio is absent.

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossbeam_channel::{Sender, unbounded};
use rodio::buffer::SamplesBuffer;
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};
use thiserror::Error;
use tracing::{debug, info, warn};

use crate::settings::SettingsStore;

/// Drop the cached `OutputStream` after this much idle time so the
/// next play picks up the (possibly changed) default audio device.
/// 30 s is well above any plausible "pause-resume burst" cadence,
/// so rapid hotkey use stays on the warm cached stream.
const STREAM_IDLE_REFRESH: Duration = Duration::from_secs(30);

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("could not initialise the audio output: {0}")]
    Init(String),
}

#[derive(Debug, Clone, Copy)]
pub enum SoundEvent {
    Correct,
    Pause,
    Resume,
}

impl SoundEvent {
    fn file_name(self) -> &'static str {
        match self {
            Self::Correct => "correct.ogg",
            Self::Pause => "pause.ogg",
            Self::Resume => "resume.ogg",
        }
    }

    /// Synthesised fallback tone parameters: `(frequency_Hz,
    /// duration_ms)`. Used when the user's theme dir doesn't ship a
    /// matching `.ogg` file. Generating tones at runtime instead of
    /// shipping audio assets keeps the binary small and avoids
    /// per-platform decoder quirks.
    ///
    /// Distinct pitches per event give the user audible feedback
    /// without having to look at the tray.
    fn synth_tone(self) -> (f32, u64) {
        match self {
            // Bright "ping" — a correction was applied.
            Self::Correct => (880.0, 90),
            // Lower, longer — auto-switching went off.
            Self::Pause => (440.0, 140),
            // Mid pitch, short — auto-switching is back on.
            Self::Resume => (660.0, 90),
        }
    }
}

#[derive(Debug)]
enum AudioCmd {
    Play(SoundEvent),
    Refresh {
        theme_dir: Option<PathBuf>,
        volume: f32,
    },
    Shutdown,
}

/// Thin handle to the audio worker thread. `Send + Sync`, cheap to
/// clone via `Arc`.
pub struct AudioPlayer {
    cmd_tx: Sender<AudioCmd>,
}

impl AudioPlayer {
    pub fn new() -> Self {
        let (tx, rx) = unbounded::<AudioCmd>();
        let _ = std::thread::Builder::new()
            .name("kb-audio".into())
            .spawn(move || run_worker(rx));
        Self { cmd_tx: tx }
    }

    pub fn refresh_from(&self, settings: &SettingsStore) {
        let snap = settings.snapshot();
        let dir = SettingsStore::project_dirs()
            .ok()
            .map(|p| p.config_dir().join("sound-themes").join(&snap.sounds.theme));
        let _ = self.cmd_tx.send(AudioCmd::Refresh {
            theme_dir: dir,
            volume: snap.sounds.volume.clamp(0.0, 1.0),
        });
    }

    pub fn play(&self, event: SoundEvent) {
        let _ = self.cmd_tx.send(AudioCmd::Play(event));
    }
}

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(AudioCmd::Shutdown);
    }
}

impl Default for AudioPlayer {
    fn default() -> Self {
        Self::new()
    }
}

/// State owned by the audio worker thread. Not `Send` (because of
/// `OutputStream`); never escapes the worker.
struct WorkerState {
    /// Lazily-opened, long-lived stream. `None` means "not yet opened
    /// or just invalidated; open on next play". `last_opened` is set
    /// when the current `stream` was created — used to time the idle
    /// refresh.
    stream: Option<(OutputStream, OutputStreamHandle)>,
    last_opened: Instant,
    theme_dir: Option<PathBuf>,
    volume: f32,
}

impl WorkerState {
    fn new() -> Self {
        Self {
            stream: None,
            last_opened: Instant::now(),
            theme_dir: None,
            volume: 0.6,
        }
    }

    /// Get a usable handle, opening (or re-opening) the stream as
    /// needed. Returns `None` if the OS refuses to give us a default
    /// device — caller must treat this as "no audio available right
    /// now" and skip the play silently.
    fn handle(&mut self) -> Option<OutputStreamHandle> {
        // Refresh after long idle so default-device changes (BT
        // headphones, HDMI, OS sound-settings change) are eventually
        // picked up. Stream creation is what costs ~20-50 ms; reuse
        // is essentially free.
        if self
            .stream
            .as_ref()
            .is_some_and(|_| self.last_opened.elapsed() > STREAM_IDLE_REFRESH)
        {
            debug!("audio: dropping stale OutputStream after idle refresh window");
            self.stream = None;
        }
        if self.stream.is_none() {
            match OutputStream::try_default() {
                Ok((s, h)) => {
                    self.stream = Some((s, h));
                    self.last_opened = Instant::now();
                }
                Err(e) => {
                    warn!(err = %e, "could not open default audio device; play skipped");
                    return None;
                }
            }
        }
        self.stream.as_ref().map(|(_, h)| h.clone())
    }

    /// Drop the cached stream so the next `handle()` call reopens
    /// from scratch. Used when a play fails: the device may have
    /// gone away (USB unplugged, BT disconnect, OS audio service
    /// restart) and we don't want to keep retrying against a dead
    /// handle.
    fn invalidate(&mut self) {
        self.stream = None;
    }
}

fn run_worker(rx: crossbeam_channel::Receiver<AudioCmd>) {
    info!("audio worker started (cached OutputStream + idle refresh)");

    let mut state = WorkerState::new();

    while let Ok(cmd) = rx.recv() {
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
fn play_event(state: &mut WorkerState, event: SoundEvent) {
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
fn play_with_handle(
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

fn play_file(
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
fn play_tone(
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

/// Build a single-channel `SamplesBuffer` for the given tone. Layout:
///
/// ```text
///   [LEAD silence] [fade-in] [body] [fade-out] [TAIL silence]
///   |----30ms----| |--10ms--|       |--25ms--| |----60ms----|
/// ```
///
/// Why both ramp envelopes AND silence padding:
///
/// * **Ramp envelopes** (10 ms in, 25 ms out) prevent sample-level
///   discontinuity at sine wave start / end. Cutting a sine mid-cycle
///   is a hard-edge step the speaker reproduces as a click.
/// * **Lead silence** absorbs OS audio-stack warmup the *first* time
///   we hand audio to a freshly-opened `OutputStream` (or to one
///   that's been idle long enough to have unrouted itself on some
///   platforms). Without it, the fade-in is partly eaten by device
///   init and the user hears a tone that begins mid-rise.
/// * **Tail silence** gives the OS audio buffer time to flush the
///   final real samples before `sleep_until_end` returns and the
///   sink drops; with the long-lived stream design that flush time
///   is short, but the cushion costs nothing audible.
///
/// Why we render the envelope by hand instead of using rodio's
/// chainable `fade_in` / `fade_out`: rodio 0.20's `Source::fade_out`
/// starts ramping at sample 0 and reaches silence at `duration` —
/// the opposite of what the name suggests. Computing the envelope
/// ourselves is safer and locks the behaviour into our own tests.
fn synthesise_blip(freq: f32, ms: u64, amp: f32, sample_rate: u32) -> SamplesBuffer<f32> {
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

const LEAD_SILENCE_MS: u64 = 30;
const TAIL_SILENCE_MS: u64 = 60;

#[cfg(test)]
mod tests {
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
            let expected =
                (u64::from(sr) * (LEAD_SILENCE_MS + ms + TAIL_SILENCE_MS) / 1000) as usize;
            assert_eq!(samples.len(), expected, "ms = {ms}");
        }
    }
}
