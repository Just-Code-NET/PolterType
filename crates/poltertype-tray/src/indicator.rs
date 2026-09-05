//! The Linux tray, driven straight through libayatana-appindicator.
//!
//! `tray-icon` builds the very same object here, and keeping it would
//! be the obvious thing — except that its `set_tooltip` is an empty
//! function on Linux, in every release including 0.24, five ahead of
//! ours. So every state change wrote a tooltip that had nowhere to go
//! ([#59](https://github.com/Just-Code-NET/PolterType/issues/59)).
//!
//! The library underneath does implement one. A StatusNotifierItem
//! host draws the hover text from the item's `ToolTip` property — KDE
//! reads nothing else for it — and `app_indicator_set_tooltip_full` is
//! what fills that property in. Reaching it needs the `AppIndicator`
//! pointer, which `tray-icon` keeps private, so the indicator is ours
//! here and `tray-icon` stays the tray on Windows and macOS.
//!
//! Everything else is deliberately a transcription of what
//! `tray-icon`'s GTK backend does — same library, same calls, in the
//! same order — so that the desktops it already works on cannot tell
//! the difference.

use std::cell::{Cell, RefCell};
use std::ffi::CString;
use std::path::{Path, PathBuf};

use glib::translate::ToGlibPtr;
use libappindicator_sys::{
    AppIndicator, AppIndicatorCategory_APP_INDICATOR_CATEGORY_APPLICATION_STATUS,
    AppIndicatorStatus_APP_INDICATOR_STATUS_ACTIVE,
    AppIndicatorStatus_APP_INDICATOR_STATUS_PASSIVE, LIB, app_indicator_new_with_path,
    app_indicator_set_icon_full, app_indicator_set_icon_theme_path, app_indicator_set_menu,
    app_indicator_set_status, app_indicator_set_title, gchar,
};
use tracing::debug;
use tray_icon::menu::ContextMenu;

use crate::{Icon, TrayError};

/// The item's own id on the bus, and the last path component of the
/// D-Bus object a tray host talks to. `tray-icon` hardcodes
/// `"tray-icon tray app"`, which every application built on it then
/// shares; ours says who we are.
const INDICATOR_ID: &str = "poltertype";

/// Where the icon a tray host reads back is written. Under
/// `XDG_RUNTIME_DIR` when there is one — tmpfs, cleaned at logout.
const ICON_DIR: &str = "poltertype";

/// Stem of the icon file. The counter appended to it is what makes a
/// redraw visible: hosts cache by icon name, so the name has to change
/// even though the path's meaning does not.
const ICON_STEM: &str = "tray";

/// The tooltip setter, absent from the pre-Ayatana `libappindicator3`.
const TOOLTIP_SYMBOL: &[u8] = b"app_indicator_set_tooltip_full\0";

/// What a tray host calls the item in a list. Spelt out here rather
/// than taken from the binary: the product name is fixed in every
/// language, and a parameter for it would have to be ignored on the
/// two platforms where the same call writes into the menu bar.
const APP_TITLE: &str = "PolterType";

/// `app_indicator_set_tooltip_full`: indicator, icon name, title, body.
type SetTooltip = unsafe extern "C" fn(*mut AppIndicator, *const gchar, *const gchar, *const gchar);

/// The system tray icon, its menu and its tooltip.
///
/// Not `Send`: every call below reaches GTK, which owns this thread.
/// The raw pointer field says so to the compiler as well.
pub struct Tray {
    indicator: *mut AppIndicator,
    /// Held for its lifetime, not read. The indicator takes a GTK
    /// reference on the menu widget, but the widget's items belong to
    /// this `muda` menu — drop it and the tray keeps a menu whose rows
    /// have been freed.
    _menu: Box<dyn ContextMenu>,
    dir: PathBuf,
    icon: RefCell<PathBuf>,
    counter: Cell<u32>,
    /// `None` on a system carrying the original `libappindicator3`,
    /// which has no tooltip API at all. Looked up once so that a tray
    /// refresh cannot log the same absence forever, and kept as a bare
    /// pointer rather than a `Symbol` because the handle it would
    /// borrow from belongs to `libappindicator-sys`'s copy of
    /// `libloading`, not ours. Sound either way: that handle is a
    /// `static` the process never closes.
    tooltip: Option<SetTooltip>,
}

impl Tray {
    /// Build the tray: menu, icon and tooltip, visible immediately.
    ///
    /// # Errors
    ///
    /// [`TrayError::Io`] when the icon cannot be written to disk.
    pub fn new(menu: Box<dyn ContextMenu>, icon: Icon, tooltip: &str) -> Result<Self, TrayError> {
        let dir = icon_dir();
        let path = write_icon(&dir, 0, icon)?;
        let id = c_string(INDICATOR_ID);
        let theme = c_string(&dir.to_string_lossy());
        let name = c_string(&path.to_string_lossy());
        let desc = c_string(APP_TITLE);
        let title = c_string(APP_TITLE);

        // SAFETY: every pointer is a NUL-terminated string that
        // outlives the call, and the indicator returned is owned by
        // this struct from here on. The order is `tray-icon`'s: create
        // with a theme path, then status, icon, title, menu.
        let indicator = unsafe {
            let indicator = app_indicator_new_with_path(
                id.as_ptr(),
                name.as_ptr(),
                AppIndicatorCategory_APP_INDICATOR_CATEGORY_APPLICATION_STATUS,
                theme.as_ptr(),
            );
            app_indicator_set_status(indicator, AppIndicatorStatus_APP_INDICATOR_STATUS_ACTIVE);
            app_indicator_set_icon_full(indicator, name.as_ptr(), desc.as_ptr());
            // Not the tooltip: `Title` is what a host puts beside the
            // icon in its "hidden items" list, and it would otherwise
            // fall back to the process name, lower-cased.
            app_indicator_set_title(indicator, title.as_ptr());
            indicator
        };

        let gtk_menu = menu.gtk_context_menu();
        // SAFETY: the widget outlives the call — the `Stash` holding
        // its pointer lives to the end of this statement and the menu
        // itself to the end of the struct — and the indicator takes
        // its own reference to it.
        unsafe { app_indicator_set_menu(indicator, gtk_menu.to_glib_none().0) };

        // SAFETY: reading a symbol out of the object this indicator
        // was created from. Nothing is called through it here.
        let tooltip_fn = unsafe { LIB.get::<SetTooltip>(TOOLTIP_SYMBOL) }
            .ok()
            .map(|symbol| *symbol);
        if tooltip_fn.is_none() {
            debug!("tray library has no tooltip API; the icon will have no hover text");
        }

        let tray = Self {
            indicator,
            _menu: menu,
            dir,
            icon: RefCell::new(path),
            counter: Cell::new(0),
            tooltip: tooltip_fn,
        };
        tray.set_tooltip(tooltip)?;
        Ok(tray)
    }

    /// Redraw the icon from a freshly rasterised buffer.
    ///
    /// # Errors
    ///
    /// [`TrayError::Io`] when the new icon cannot be written.
    pub fn set_icon(&self, icon: Icon) -> Result<(), TrayError> {
        let counter = self.counter.get().wrapping_add(1);
        let path = write_icon(&self.dir, counter, icon)?;
        let name = c_string(&path.to_string_lossy());
        let desc = c_string(APP_TITLE);
        let theme = c_string(&self.dir.to_string_lossy());
        // SAFETY: same contract as the constructor — live indicator,
        // NUL-terminated strings that outlive the call.
        unsafe {
            app_indicator_set_icon_theme_path(self.indicator, theme.as_ptr());
            app_indicator_set_icon_full(self.indicator, name.as_ptr(), desc.as_ptr());
        }
        self.counter.set(counter);
        let previous = self.icon.replace(path);
        let _ = std::fs::remove_file(previous);
        Ok(())
    }

    /// Set the hover text a tray host shows for the icon.
    ///
    /// # Errors
    ///
    /// Never, today: a library with no tooltip API is reported once at
    /// construction and silently skipped afterwards, because there is
    /// nothing the user could do about it on every refresh.
    pub fn set_tooltip(&self, text: &str) -> Result<(), TrayError> {
        let Some(set) = self.tooltip else {
            return Ok(());
        };
        let title = c_string(text);
        // SAFETY: the symbol came from the object holding this
        // indicator, and both pointers outlive the call. A null icon
        // and body leave the tooltip to its title, which is the only
        // part KDE draws for a panel item.
        unsafe {
            set(
                self.indicator,
                std::ptr::null(),
                title.as_ptr(),
                std::ptr::null(),
            )
        };
        Ok(())
    }

    /// Show or hide the icon without tearing the tray down, so that
    /// turning it back on stays a config change rather than a restart.
    ///
    /// # Errors
    ///
    /// Never; the signature matches the other backends.
    pub fn set_visible(&self, visible: bool) -> Result<(), TrayError> {
        let status = if visible {
            AppIndicatorStatus_APP_INDICATOR_STATUS_ACTIVE
        } else {
            AppIndicatorStatus_APP_INDICATOR_STATUS_PASSIVE
        };
        // SAFETY: live indicator, plain enum value.
        unsafe { app_indicator_set_status(self.indicator, status) };
        Ok(())
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        // SAFETY: the indicator is still ours and still live.
        unsafe {
            app_indicator_set_status(
                self.indicator,
                AppIndicatorStatus_APP_INDICATOR_STATUS_PASSIVE,
            )
        };
        let _ = std::fs::remove_file(self.icon.borrow().as_path());
        // The indicator itself is deliberately not unreferenced: GTK
        // may still hold it while the loop unwinds, and `tray-icon`
        // leaks it here too. The process is on its way out.
    }
}

/// Where the icon file lives: `$XDG_RUNTIME_DIR/poltertype`, or the
/// temp directory when the session sets no runtime dir.
fn icon_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(std::env::temp_dir)
        .join(ICON_DIR)
}

/// Write `icon` as a PNG the indicator can read back, returning its
/// path. `counter` only has to differ from the last one.
fn write_icon(dir: &Path, counter: u32, icon: Icon) -> Result<PathBuf, TrayError> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{ICON_STEM}-{counter}.png"));
    let (width, height) = (icon.width(), icon.height());
    let rgba = icon.into_rgba();
    let file = std::io::BufWriter::new(std::fs::File::create(&path)?);
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|e| TrayError::Backend(e.to_string()))?;
    writer
        .write_image_data(&rgba)
        .map_err(|e| TrayError::Backend(e.to_string()))?;
    Ok(path)
}

/// A C string that cannot fail to be one. An interior NUL would only
/// arrive from a translated catalog, and losing a byte of a tooltip
/// beats losing the tooltip.
fn c_string(s: &str) -> CString {
    CString::new(s.replace('\0', " ")).unwrap_or_default()
}
