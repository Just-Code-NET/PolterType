//! Routing a menu click to the plug-in it belongs to.

use tracing::warn;
use tray_icon::menu::MenuId;

use crate::plugins::supervisor::{run_command, run_command_for_row};

use super::state::PluginMenu;

impl PluginMenu {
    /// Handle a menu click if it belongs to a plug-in. Returns whether
    /// it did, so the caller can stop looking.
    pub fn handle(&mut self, id: &MenuId) -> bool {
        if let Some((index, command, row)) = self.row_routes.get(id).cloned() {
            let Some(ext) = self.extensions.get(index) else {
                return false;
            };
            let outcome = if row.is_empty() {
                run_command(ext, &command)
            } else {
                run_command_for_row(ext, &command, &row)
            };
            if let Err(e) = outcome {
                warn!(id = %ext.id, "plug-in list entry failed: {e}");
            }
            // Acting on a row usually removes it, and a stale row is
            // worse here than a stale tick: clicking it again would act
            // on something that is gone.
            std::thread::sleep(REFRESH_SETTLE);
            self.refresh();
            return true;
        }
        let Some((index, command)) = self.routes.get(id).cloned() else {
            return false;
        };
        let Some(ext) = self.extensions.get(index) else {
            return false;
        };
        if let Err(e) = run_command(ext, &command) {
            warn!(id = %ext.id, "plug-in menu entry failed: {e}");
        }

        // The click almost certainly changed what the menu should show,
        // and this is the one moment we know to look. The command is
        // spawned rather than waited on, so its state may not have landed
        // yet — hence `REFRESH_SETTLE`, and hence `refresh` staying public
        // for the periodic caller.
        std::thread::sleep(REFRESH_SETTLE);
        self.refresh();
        true
    }
}

/// How long to let a just-launched command finish before re-reading
/// state.
///
/// A menu click spawns the command without waiting, so reading back
/// immediately races it and shows the value the user just replaced.
/// Bounded to something nobody perceives as a hang, since this is on
/// the UI thread; the periodic refresh corrects a slower command anyway.
const REFRESH_SETTLE: std::time::Duration = std::time::Duration::from_millis(250);
