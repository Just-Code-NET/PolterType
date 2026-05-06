//! Sound effects via `rodio`, owned by a dedicated worker thread.
//!
//! Why a worker thread: rodio's `OutputStream` is `!Send` on most
//! platforms because the underlying audio API (CoreAudio, ALSA, …)
//! ties a stream to its creating thread. Wrapping it behind a
//! crossbeam channel keeps `AudioPlayer` `Send + Sync` so the engine
//! can hold an `Arc<AudioPlayer>` on its own thread.
//!
//! Why we open a fresh `OutputStream` *per play*: the OS default
//! audio device changes mid-session — typically when the user
//! plugs / unplugs Bluetooth headphones, switches HDMI outputs, or
//! suspends and resumes. A long-lived `OutputStream` cached at
//! startup keeps writing to the no-longer-default device and the
//! user hears nothing. Opening a stream per event always picks up
//! the current default. Init costs ~10–50 ms on Windows / macOS and
//! a few ms on Linux/PulseAudio — well under the duration of even
//! the shortest tone we play, so it's invisible to the user.
//!
//! Themes live in `<config-dir>/sound-themes/<name>/<event>.ogg`.
//! Missing files are silent — we never crash because audio is absent.

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use crossbeam_channel::{Sender, unbounded};
use rodio::buffer::SamplesBuffer;
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};
use thiserror::Error;
use tracing::{debug, info, warn};

use crate::settings::SettingsStore;

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

fn run_worker(rx: crossbeam_channel::Receiver<AudioCmd>) {
    info!("audio worker started (per-play stream allocation)");

    let mut theme_dir: Option<PathBuf> = None;
    let mut volume: f32 = 0.6;

    while let Ok(cmd) = rx.recv() {
        match cmd {
            AudioCmd::Refresh {
                theme_dir: d,
                volume: v,
            } => {
                theme_dir = d;
                volume = v;
                debug!(?theme_dir, volume, "audio refreshed");
            }
            AudioCmd::Play(event) => {
                // Prefer a user-supplied theme file if it exists;
                // otherwise fall back to a synthesised tone so the
                // user always hears *something*.
                if let Some(dir) = theme_dir.as_ref() {
                    let path = dir.join(event.file_name());
                    if path.exists() {
                        if let Err(e) = play_file(&path, volume) {
                            warn!(?path, err = %e, "could not play theme sound");
                        }
                        continue;
                    }
                }
                if let Err(e) = play_tone(event, volume) {
                    warn!(?e, "could not play synthesised tone");
                }
            }
            AudioCmd::Shutdown => break,
        }
    }
    info!("audio worker stopped");
}

/// Open a fresh OutputStream against the current OS default device.
/// Doing this on every play means we automatically follow Bluetooth
/// connect/disconnect, HDMI switches, suspend/resume, default-device
/// changes from the OS sound settings — without subscribing to any
/// platform-specific device-change notifications.
fn fresh_stream()
-> Result<(OutputStream, OutputStreamHandle), Box<dyn std::error::Error + Send + Sync>> {
    OutputStream::try_default().map_err(Into::into)
}

fn play_file(
    path: &std::path::Path,
    volume: f32,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (_stream, handle) = fresh_stream()?;
    let file = BufReader::new(File::open(path)?);
    let decoder = Decoder::new(file)?;
    let sink = Sink::try_new(&handle)?;
    sink.set_volume(volume);
    sink.append(decoder);
    // We OWN the stream for this call; dropping it before playback
    // ends would cut the sound off. Block the audio worker thread
    // until the sink drains. The worker thread is dedicated to us
    // so this doesn't stall any keystroke handling.
    sink.sleep_until_end();
    Ok(())
}

/// Play a synthesised fallback tone for `event`. Pre-rendered as a
/// `SamplesBuffer` with a fade-in *and* fade-out envelope so the
/// playback boundaries are smooth.
///
/// Why we render the envelope by hand instead of using rodio's
/// chainable `fade_in` / `fade_out` adapters: rodio 0.20's
/// `Source::fade_out` actually starts ramping at sample 0 and
/// reaches silence at `duration`, which is the opposite of what the
/// name suggests — the rest of the tone plays at zero amplitude.
/// Computing the envelope ourselves lets us shape a real
/// attack-sustain-release: 10-ms ramp up, body, 25-ms ramp down to
/// silence at the very end.
///
/// The trailing taper is what fixes the "broken / glitchy" click on
/// rapid pause/resume — cutting a sine mid-cycle leaves a sample-
/// level discontinuity that the speaker reproduces as a hard pop.
fn play_tone(
    event: SoundEvent,
    volume: f32,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (_stream, handle) = fresh_stream()?;
    let (freq, ms) = event.synth_tone();
    let amp = (volume * 0.4).clamp(0.0, 1.0);
    let source = synthesise_blip(freq, ms, amp, /* sample_rate */ 44_100);
    let sink = Sink::try_new(&handle)?;
    sink.append(source);
    // Same reason as play_file: must keep the stream alive until the
    // tone has actually been written out.
    sink.sleep_until_end();
    Ok(())
}

/// Build a single-channel `SamplesBuffer` containing a `freq`-Hz sine
/// wave of `ms` milliseconds, with linear fade-in (10 ms) and
/// fade-out (25 ms). Both ramps are clamped to a third of the total
/// duration each so they never overlap on unusually short events.
fn synthesise_blip(freq: f32, ms: u64, amp: f32, sample_rate: u32) -> SamplesBuffer<f32> {
    let total = ((u64::from(sample_rate)).saturating_mul(ms) / 1000) as usize;
    let cap = total / 3;
    let fade_in = ((u64::from(sample_rate) * 10) / 1000) as usize;
    let fade_out = ((u64::from(sample_rate) * 25) / 1000) as usize;
    let fade_in = fade_in.min(cap);
    let fade_out = fade_out.min(cap);

    let two_pi_f = 2.0 * std::f32::consts::PI * freq;
    let inv_sr = 1.0 / sample_rate as f32;

    // Envelope: ramp from 0.0 at sample 0 to 1.0 at sample
    // `fade_in - 1`, hold at 1.0 through the body, then ramp from 1.0
    // back to exactly 0.0 at sample `total - 1`. Anchoring the
    // boundary samples to silence is what kills the speaker click —
    // a `1/fade_out` final amplitude is small but still reproduces
    // as a faint pop on some speakers.
    let mut samples = Vec::with_capacity(total);
    for i in 0..total {
        let envelope = if fade_in > 1 && i < fade_in {
            i as f32 / (fade_in - 1) as f32
        } else if fade_out > 1 && i >= total.saturating_sub(fade_out) {
            let from_end = total - 1 - i;
            from_end as f32 / (fade_out - 1) as f32
        } else {
            1.0
        };
        let t = i as f32 * inv_sr;
        let v = (two_pi_f * t).sin() * envelope * amp;
        samples.push(v);
    }
    SamplesBuffer::new(1, sample_rate, samples)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The blip must start and end at (very near) zero amplitude —
    /// otherwise the speaker reproduces the discontinuity as a click,
    /// which is exactly the regression we're guarding against.
    #[test]
    fn synthesised_blip_starts_and_ends_silent() {
        let buf = synthesise_blip(440.0, 140, 1.0, 44_100);
        let samples: Vec<f32> = buf.collect();
        assert!(!samples.is_empty());
        assert!(
            samples.first().unwrap().abs() < 1e-3,
            "first sample should be silent, got {}",
            samples.first().unwrap()
        );
        assert!(
            samples.last().unwrap().abs() < 1e-3,
            "last sample should be silent, got {}",
            samples.last().unwrap()
        );
    }

    /// The body should reach peak amplitude — i.e. the envelope must
    /// not collapse the whole signal (the rodio `fade_out` bug we
    /// went around earlier did exactly that for ms > fade_dur).
    #[test]
    fn synthesised_blip_reaches_full_amplitude() {
        let buf = synthesise_blip(440.0, 140, 1.0, 44_100);
        let peak = buf.map(f32::abs).fold(0.0_f32, f32::max);
        assert!(peak > 0.95, "expected peak ≈ 1.0, got {peak}");
    }

    /// Even a 30-ms event (shorter than fade_in + fade_out together)
    /// must still produce non-empty audio with capped ramps. This
    /// locks in the `cap = total / 3` clamp.
    #[test]
    fn synthesised_blip_handles_very_short_durations() {
        let buf = synthesise_blip(440.0, 30, 1.0, 44_100);
        let samples: Vec<f32> = buf.collect();
        assert_eq!(samples.len(), 44_100 * 30 / 1000);
        assert!(samples.first().unwrap().abs() < 1e-3);
        assert!(samples.last().unwrap().abs() < 1e-3);
    }
}
