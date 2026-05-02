//! Sound effects via `rodio`, owned by a dedicated worker thread.
//!
//! Why a worker thread: rodio's `OutputStream` is `!Send` on most
//! platforms because the underlying audio API (CoreAudio, ALSA, …)
//! ties a stream to its creating thread. Wrapping it behind a
//! crossbeam channel keeps `AudioPlayer` `Send + Sync` so the engine
//! can hold an `Arc<AudioPlayer>` on its own thread.
//!
//! Themes live in `<config-dir>/sound-themes/<name>/<event>.ogg`.
//! Missing files are silent — we never crash because audio is absent.

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::time::Duration;

use crossbeam_channel::{Sender, unbounded};
use rodio::source::{SineWave, Source};
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
    let (stream, handle) = match OutputStream::try_default() {
        Ok((s, h)) => (Some(s), Some(h)),
        Err(e) => {
            warn!(err = %e, "no audio output available; sounds disabled");
            (None, None)
        }
    };
    info!(audio = handle.is_some(), "audio worker started");

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
                let Some(handle) = handle.as_ref() else {
                    continue;
                };
                // Prefer a user-supplied theme file if it exists;
                // otherwise fall back to a synthesised tone so the
                // user always hears *something*.
                if let Some(dir) = theme_dir.as_ref() {
                    let path = dir.join(event.file_name());
                    if path.exists() {
                        if let Err(e) = play_file(handle, &path, volume) {
                            warn!(?path, err = %e, "could not play theme sound");
                        }
                        continue;
                    }
                }
                if let Err(e) = play_tone(handle, event, volume) {
                    warn!(?e, "could not play synthesised tone");
                }
            }
            AudioCmd::Shutdown => break,
        }
    }
    drop(stream);
    info!("audio worker stopped");
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
    sink.detach();
    Ok(())
}

/// Play a synthesised fallback tone for `event`. Sine wave shaped
/// with a brief amplitude envelope so it sounds like a soft "blip"
/// rather than a click → pop → click. Cheap and asset-free.
fn play_tone(
    handle: &OutputStreamHandle,
    event: SoundEvent,
    volume: f32,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (freq, ms) = event.synth_tone();
    let dur = Duration::from_millis(ms);
    let source = SineWave::new(freq)
        // Soft fade-in/out to take the edge off the square boundary.
        .take_duration(dur)
        .fade_in(Duration::from_millis(10))
        .amplify((volume * 0.4).clamp(0.0, 1.0));
    let sink = Sink::try_new(handle)?;
    sink.append(source);
    sink.detach();
    Ok(())
}
