//! Which section of the pane is on screen, and which controls that puts
//! in view.

use poltertype_core::plugins::ControlKind;

use super::pane::PluginPane;

impl PluginPane {
    /// Every section heading, in declaration order.
    pub fn sections(&self) -> Vec<usize> {
        self.ext
            .manifest
            .pane
            .iter()
            .enumerate()
            .filter(|(_, c)| c.kind == ControlKind::Section)
            .map(|(i, _)| i)
            .collect()
    }

    /// The section on screen: what was chosen, or the first one.
    pub fn selected_section(&self) -> Option<usize> {
        match self.section {
            Some(i) if matches!(self.control(i).map(|c| c.kind), Some(ControlKind::Section)) => {
                Some(i)
            }
            _ => self.sections().first().copied(),
        }
    }

    /// Is this control on screen?
    ///
    /// A control belongs to the nearest [`ControlKind::Section`] above
    /// it. Controls declared *before* the first section belong to none
    /// and are always shown, which is also what makes a plug-in with no
    /// sections render everything.
    pub fn is_visible(&self, index: usize) -> bool {
        let controls = &self.ext.manifest.pane;
        let Some(selected) = self.selected_section() else {
            return true;
        };
        if index == selected {
            return true;
        }
        if matches!(
            controls.get(index).map(|c| c.kind),
            Some(ControlKind::Section)
        ) {
            return false;
        }
        controls[..index.min(controls.len())]
            .iter()
            .enumerate()
            .rev()
            .find(|(_, c)| c.kind == ControlKind::Section)
            .is_none_or(|(i, _)| i == selected)
    }

    /// Show one section.
    ///
    /// Also the moment to re-read the arrays: reaching a section is the
    /// user's own step, and it is where a change made in an editor
    /// since the window opened gets picked up.
    pub fn select_section(&mut self, index: usize) {
        self.section = Some(index);
        self.reload_arrays();
    }
}
