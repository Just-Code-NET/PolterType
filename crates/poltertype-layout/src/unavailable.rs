//! The switcher used when there is no switcher.

use crate::*;

/// A [`LayoutSwitcher`] that answers "not here" to everything.
///
/// Every method returning an error is the point: the app is worth
/// running without one — it still watches the keyboard, still knows
/// which word came out wrong, still opens Settings and its Setup pane,
/// which is where the user finds out why nothing is being switched.
/// Exiting instead means an autostarted PolterType that dies at login
/// on a session that was merely slow to come up, leaving nothing behind
/// but an exit code.
pub struct UnavailableSwitcher;

impl LayoutSwitcher for UnavailableSwitcher {
    fn current(&self) -> Result<LayoutId, LayoutError> {
        Err(unavailable())
    }

    fn list_active(&self) -> Result<Vec<LayoutId>, LayoutError> {
        Err(unavailable())
    }

    fn switch_to(&self, _id: &LayoutId) -> Result<(), LayoutError> {
        Err(unavailable())
    }

    fn backend_name(&self) -> &'static str {
        "unavailable"
    }
}

fn unavailable() -> LayoutError {
    LayoutError::Unsupported(
        "no layout-switching backend on this session; see the Setup pane".to_owned(),
    )
}

/// [`crate::factory::create_switcher`]'s answer on a `target_os` none of
/// the three real backends compile for. Dead code on every OS this
/// workspace actually builds for (linux/macos/windows) — kept so an
/// exotic Unix still links instead of failing to compile.
#[allow(dead_code)]
pub fn create_switcher() -> Result<Box<dyn LayoutSwitcher>, LayoutError> {
    Err(LayoutError::Unsupported(format!(
        "unsupported target_os = {}",
        std::env::consts::OS
    )))
}
