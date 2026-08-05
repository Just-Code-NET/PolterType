//! Timing knobs for the audio worker.

use std::time::Duration;

/// Drop the cached `OutputStream` after this much idle time. Two
/// reasons: the next play picks up the (possibly changed) default
/// audio device, and — critically on macOS with HDMI output — an
/// idle open stream otherwise keeps coreaudiod's power assertion
/// alive and blocks display/system sleep.
/// 30 s is well above any plausible "pause-resume burst" cadence,
/// so rapid hotkey use stays on the warm cached stream.
pub(crate) const STREAM_IDLE_REFRESH: Duration = Duration::from_secs(30);

pub(crate) const LEAD_SILENCE_MS: u64 = 30;

pub(crate) const TAIL_SILENCE_MS: u64 = 60;
