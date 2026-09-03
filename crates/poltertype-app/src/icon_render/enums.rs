//! Which way a `mono` icon has to read.

use super::*;

/// Which way round a `mono` icon has to read.
///
/// Sampled once, from the desktop's own dark/light preference — a
/// panel is free to disagree, which is what the halo is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelPolarity {
    Dark,
    Light,
}

impl PanelPolarity {
    pub fn from_prefers_dark(dark: bool) -> Self {
        if dark { Self::Dark } else { Self::Light }
    }

    pub(super) fn letters(self) -> [u8; 4] {
        match self {
            Self::Dark => MONO_ON_DARK,
            Self::Light => MONO_ON_LIGHT,
        }
    }

    pub(super) fn halo(self) -> [u8; 4] {
        match self {
            Self::Dark => MONO_HALO_ON_DARK,
            Self::Light => MONO_HALO_ON_LIGHT,
        }
    }
}
