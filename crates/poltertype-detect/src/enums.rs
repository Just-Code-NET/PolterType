//! Verdict and classification enums shared across detectors.

use poltertype_types::DetectionVerdict;
use serde::{Deserialize, Serialize};

/// What a [`Detector`] decided.
#[derive(Debug, Clone)]
pub enum Verdict {
    /// No opinion — try the next detector.
    NoOpinion,
    /// Strong veto: leave the buffer alone, even if later detectors
    /// would suggest a switch.
    Keep { reason: String },
    /// Switch the active layout to the named one.
    Switch(DetectionVerdict),
}

#[derive(Debug, Clone)]
pub enum RewriteVerdict {
    Keep,
    Replace {
        text: String,
        reason: String,
        require_confirmation: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum Script {
    Latin,
    Cyrillic,
    Greek,
    Armenian,
    Hebrew,
    Arabic,
    Other,
}

impl Script {
    pub fn of(c: char) -> Self {
        let cp = c as u32;
        match cp {
            0x0041..=0x005A | 0x0061..=0x007A => Self::Latin,
            0x00C0..=0x024F => Self::Latin,
            0x0400..=0x052F => Self::Cyrillic,
            0x0370..=0x03FF => Self::Greek,
            0x0530..=0x058F => Self::Armenian,
            0x0590..=0x05FF => Self::Hebrew,
            0x0600..=0x06FF => Self::Arabic,
            _ => Self::Other,
        }
    }
}
