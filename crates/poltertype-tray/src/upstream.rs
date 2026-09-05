//! The tray on Windows and macOS, where `tray-icon` needs no help.
//!
//! Both platforms implement the tooltip that Linux does not, so this
//! is a thin pass-through whose only job is to keep the binary talking
//! to one tray type instead of two.

use tray_icon::menu::ContextMenu;
use tray_icon::{TrayIcon, TrayIconBuilder};

use crate::{Icon, TrayError};

/// The system tray icon, its menu and its tooltip.
pub struct Tray {
    inner: TrayIcon,
}

impl Tray {
    /// Build the tray: menu, icon and tooltip, visible immediately.
    ///
    /// # Errors
    ///
    /// [`TrayError::Backend`] when the platform refuses the icon or
    /// the tray itself.
    pub fn new(menu: Box<dyn ContextMenu>, icon: Icon, tooltip: &str) -> Result<Self, TrayError> {
        let inner = TrayIconBuilder::new()
            .with_menu(menu)
            .with_tooltip(tooltip)
            .with_icon(platform_icon(icon)?)
            .build()
            .map_err(|e| TrayError::Backend(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Redraw the icon from a freshly rasterised buffer.
    ///
    /// # Errors
    ///
    /// [`TrayError::Backend`] when the platform refuses the icon.
    pub fn set_icon(&self, icon: Icon) -> Result<(), TrayError> {
        self.inner
            .set_icon(Some(platform_icon(icon)?))
            .map_err(|e| TrayError::Backend(e.to_string()))
    }

    /// Set the hover text a tray host shows for the icon.
    ///
    /// # Errors
    ///
    /// [`TrayError::Backend`] when the platform refuses the text.
    pub fn set_tooltip(&self, text: &str) -> Result<(), TrayError> {
        self.inner
            .set_tooltip(Some(text))
            .map_err(|e| TrayError::Backend(e.to_string()))
    }

    /// Show or hide the icon without tearing the tray down, so that
    /// turning it back on stays a config change rather than a restart.
    ///
    /// # Errors
    ///
    /// [`TrayError::Backend`] when the platform refuses.
    pub fn set_visible(&self, visible: bool) -> Result<(), TrayError> {
        self.inner
            .set_visible(visible)
            .map_err(|e| TrayError::Backend(e.to_string()))
    }
}

/// Hand the pixels to `tray-icon` in the shape it wants them.
fn platform_icon(icon: Icon) -> Result<tray_icon::Icon, TrayError> {
    let (width, height) = (icon.width(), icon.height());
    tray_icon::Icon::from_rgba(icon.into_rgba(), width, height)
        .map_err(|e| TrayError::Backend(e.to_string()))
}
