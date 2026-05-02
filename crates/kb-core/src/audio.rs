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
use std::io::{BufReader, Cursor};
use std::path::PathBuf;

use crossbeam_channel::{Sender, unbounded};
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

    /// Bundled placeholder bytes for the default theme. v0.1 is silent
    /// when no theme file is present; Phase 8 will swap real audio in.
    fn bundled_bytes(self) -> Option<&'static [u8]> {
        if BUNDLED_DEFAULT_OGG.is_empty() {
            None
        } else {
            Some(BUNDLED_DEFAULT_OGG)
        }
    }
}

const BUNDLED_DEFAULT_OGG: &[u8] = &[];

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
                if let Some(dir) = theme_dir.as_ref() {
                    let path = dir.join(event.file_name());
                    if path.exists() {
                        if let Err(e) = play_file(handle, &path, volume) {
                            warn!(?path, err = %e, "could not play theme sound");
                        }
                        continue;
                    }
                }
                if let Some(bytes) = event.bundled_bytes() {
                    if let Err(e) = play_bytes(handle, bytes, volume) {
                        warn!(err = %e, "could not play bundled sound");
                    }
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

fn play_bytes(
    handle: &OutputStreamHandle,
    bytes: &'static [u8],
    volume: f32,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let cursor = Cursor::new(bytes);
    let decoder = Decoder::new(cursor)?;
    let sink = Sink::try_new(handle)?;
    sink.set_volume(volume);
    sink.append(decoder);
    sink.detach();
    Ok(())
}
