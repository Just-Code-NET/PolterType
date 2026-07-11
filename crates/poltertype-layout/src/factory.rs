//! Per-OS constructor for the switcher.

use crate::*;

pub fn create_switcher() -> Result<Box<dyn LayoutSwitcher>, LayoutError> {
    #[cfg(windows)]
    {
        Ok(Box::new(windows::WindowsLayoutSwitcher::new()))
    }
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(macos::MacosLayoutSwitcher::new()))
    }
    #[cfg(target_os = "linux")]
    {
        linux::create_switcher()
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        Err(LayoutError::Unsupported(format!(
            "unsupported target_os = {}",
            std::env::consts::OS
        )))
    }
}
