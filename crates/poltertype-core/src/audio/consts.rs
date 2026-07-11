//! Timing knobs for the audio worker.

use std::time::Duration;

/// Drop the cached `OutputStream` after this much idle time so the
/// next play picks up the (possibly changed) default audio device.
/// 30 s is well above any plausible "pause-resume burst" cadence,
/// so rapid hotkey use stays on the warm cached stream.
pub(crate) const STREAM_IDLE_REFRESH: Duration = Duration::from_secs(30);

pub(crate) const LEAD_SILENCE_MS: u64 = 30;

pub(crate) const TAIL_SILENCE_MS: u64 = 60;
