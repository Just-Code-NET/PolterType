//! `AudioPlayer` — the public handle owning the worker thread.

use super::*;
use crate::settings::SettingsStore;
use crossbeam_channel::{Sender, unbounded};

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
