//! Per-OS keyboard layout switcher.
//!
//! Public surface:
//! * [`LayoutSwitcher`] — trait every per-OS implementation satisfies.
//! * [`create_switcher`] — runtime factory that picks the right backend.
//!
//! Layout-mapping tables (which key maps to which character per layout)
//! live in `data/layout-mappings/` and are loaded by `kb-detect` /
//! `kb-core`, not by this crate. We deliberately keep this crate small
//! and OS-focused.

#![deny(unsafe_op_in_unsafe_fn)]

use thiserror::Error;

pub use kb_types::LayoutId;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[derive(Debug, Error)]
pub enum LayoutError {
    #[error("the active platform does not support programmatic layout switching: {0}")]
    Unsupported(String),
    #[error("OS error while querying / switching layout: {0}")]
    Os(String),
    #[error("requested layout {0} is not currently active in the system")]
    NotActive(LayoutId),
}

pub trait LayoutSwitcher: Send + Sync {
    /// Layout currently effective for the foreground window.
    fn current(&self) -> Result<LayoutId, LayoutError>;

    /// All layouts the system knows about and the user has enabled.
    fn list_active(&self) -> Result<Vec<LayoutId>, LayoutError>;

    /// Switch the foreground window to the given layout. Must be one
    /// of the layouts returned by [`list_active`].
    fn switch_to(&self, id: &LayoutId) -> Result<(), LayoutError>;

    fn backend_name(&self) -> &'static str;
}

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
