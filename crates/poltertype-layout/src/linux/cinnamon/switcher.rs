//! The two ways into a Cinnamon session, and the probe that picks one.

use super::*;
use crate::linux::shared::cmd_exists;
use crate::linux::x11;
use crate::{LayoutError, LayoutId, LayoutSwitcher};
use tracing::{debug, warn};

/// Cinnamon 6.6+: ask the shell, exactly as its own applet does.
pub struct CinnamonSwitcher;

/// Cinnamon 6.4 and older: the X11 backend, chosen deliberately rather
/// than reached as a fallback. Wrapping it only changes the name it
/// reports, and that name is the whole point — see [`XKB_BACKEND_NAME`].
pub struct CinnamonXkbSwitcher(x11::X11Switcher);

pub fn try_init() -> Option<Box<dyn LayoutSwitcher>> {
    if !session_is_cinnamon() {
        return None;
    }
    init()
}

/// `try_init` without the session-name check — for
/// `POLTERTYPE_LAYOUT_BACKEND=cinnamon`. Which variable a display
/// manager sets, and to what, is not something we control; a user who
/// knows what they are running must not be turned away by our reading
/// of their environment. The routes below still have to work.
pub fn init_without_session_check() -> Option<Box<dyn LayoutSwitcher>> {
    init()
}

fn init() -> Option<Box<dyn LayoutSwitcher>> {
    if let Some(s) = try_init_dbus() {
        return Some(Box::new(s));
    }
    if let Some(s) = x11::try_init() {
        debug!(
            "Cinnamon without the org.Cinnamon input-source API (6.4 and older); \
             locking XKB groups, which is what its own keyboard applet does"
        );
        return Some(Box::new(CinnamonXkbSwitcher(s)));
    }
    // Falling through hands the session to the gsettings backend,
    // which on Cinnamon writes a key nobody reads. Say so: a silent
    // wrong choice here is issue #26 all over again.
    warn!(
        "Cinnamon session, but neither the org.Cinnamon input-source API nor an X11 \
         XKB group list is reachable; layout switching will probably not work"
    );
    None
}

fn try_init_dbus() -> Option<CinnamonSwitcher> {
    if !cmd_exists("gdbus") {
        warn!("Cinnamon session but `gdbus` is not in PATH; cannot ask org.Cinnamon");
        return None;
    }
    // Calling the method *is* the version check: it landed in
    // Cinnamon 6.6, and asking is more honest than parsing
    // `org.Cinnamon.CinnamonVersion` and encoding a threshold.
    match read_sources() {
        Ok(sources) if !sources.is_empty() => Some(CinnamonSwitcher),
        Ok(_) => {
            debug!("org.Cinnamon listed no input source with an XKB layout behind it");
            None
        }
        Err(e) => {
            debug!(?e, "org.Cinnamon has no input-source API here");
            None
        }
    }
}

impl LayoutSwitcher for CinnamonSwitcher {
    fn current(&self) -> Result<LayoutId, LayoutError> {
        read_sources()?
            .into_iter()
            .find(|s| s.is_current)
            .map(|s| s.layout)
            .ok_or_else(|| LayoutError::Os("Cinnamon reported no current input source".into()))
    }

    fn list_active(&self) -> Result<Vec<LayoutId>, LayoutError> {
        Ok(read_sources()?.into_iter().map(|s| s.layout).collect())
    }

    fn switch_to(&self, id: &LayoutId) -> Result<(), LayoutError> {
        let Some(source) = read_sources()?.into_iter().find(|s| &s.layout == id) else {
            return Err(LayoutError::NotActive(id.clone()));
        };
        // Cinnamon's own index, not a position in `list_active`:
        // sources we cannot map to a layout are dropped from that list
        // and would shift every index after them.
        call(ACTIVATE_INPUT_SOURCE_INDEX, &[&source.index.to_string()])?;
        debug!(layout = %id, index = source.index, "Cinnamon input source activated");
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        DBUS_BACKEND_NAME
    }
}

impl LayoutSwitcher for CinnamonXkbSwitcher {
    fn current(&self) -> Result<LayoutId, LayoutError> {
        self.0.current()
    }

    fn list_active(&self) -> Result<Vec<LayoutId>, LayoutError> {
        self.0.list_active()
    }

    fn switch_to(&self, id: &LayoutId) -> Result<(), LayoutError> {
        self.0.switch_to(id)
    }

    fn backend_name(&self) -> &'static str {
        XKB_BACKEND_NAME
    }
}
