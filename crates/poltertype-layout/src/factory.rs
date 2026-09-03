//! Per-OS constructor for the switcher.

#[cfg(target_os = "linux")]
use crate::linux as imp;
#[cfg(target_os = "macos")]
use crate::macos as imp;
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
use crate::unavailable as imp;
#[cfg(windows)]
use crate::windows as imp;

use crate::*;

pub fn create_switcher() -> Result<Box<dyn LayoutSwitcher>, LayoutError> {
    imp::create_switcher()
}
