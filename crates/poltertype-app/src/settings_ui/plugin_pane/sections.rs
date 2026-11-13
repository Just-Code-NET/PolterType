//! Which section of the pane is on screen, and which controls that puts
//! in view: the section list with its groups, the basic/advanced split,
//! and the search that cuts across both.

use poltertype_core::plugins::{ControlKind, PaneControl};

use super::pane::PluginPane;

/// One line of the section list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavEntry {
    /// A section with no group, listed on its own.
    Section(usize),
    /// Sections sharing a group, in declaration order.
    Group { label: String, sections: Vec<usize> },
}

impl PluginPane {
    /// Every section heading on offer, in declaration order. An advanced
    /// section is on offer only while advanced settings are shown.
    pub fn sections(&self) -> Vec<usize> {
        self.ext
            .manifest
            .pane
            .iter()
            .enumerate()
            .filter(|(_, c)| c.kind == ControlKind::Section && (self.advanced || !c.advanced))
            .map(|(i, _)| i)
            .collect()
    }

    /// The section list as drawn: ungrouped sections and groups, each
    /// group where its first section was declared.
    pub fn nav_entries(&self) -> Vec<NavEntry> {
        let mut entries: Vec<NavEntry> = Vec::new();
        for index in self.sections() {
            let group = self.control(index).map_or("", |c| c.group.trim());
            if group.is_empty() {
                entries.push(NavEntry::Section(index));
                continue;
            }
            let existing = entries.iter_mut().find_map(|e| match e {
                NavEntry::Group { label, sections } if label == group => Some(sections),
                _ => None,
            });
            match existing {
                Some(sections) => sections.push(index),
                None => entries.push(NavEntry::Group {
                    label: group.to_owned(),
                    sections: vec![index],
                }),
            }
        }
        entries
    }

    /// The section on screen: what was chosen, or the first one on offer.
    pub fn selected_section(&self) -> Option<usize> {
        let sections = self.sections();
        match self.section {
            Some(i) if sections.contains(&i) => Some(i),
            _ => sections.first().copied(),
        }
    }

    /// The section a control belongs to: the nearest heading above it.
    pub fn section_of(&self, index: usize) -> Option<usize> {
        let controls = &self.ext.manifest.pane;
        controls[..index.min(controls.len())]
            .iter()
            .enumerate()
            .rev()
            .find(|(_, c)| c.kind == ControlKind::Section)
            .map(|(i, _)| i)
    }

    /// "Group › Section", or the section alone.
    pub fn section_path(&self, section: usize) -> String {
        let Some(c) = self.control(section) else {
            return String::new();
        };
        if c.group.trim().is_empty() {
            c.label.clone()
        } else {
            format!("{} › {}", c.group.trim(), c.label)
        }
    }

    pub fn searching(&self) -> bool {
        !self.search.trim().is_empty()
    }

    /// Does this control answer the search? Every word typed has to
    /// appear somewhere in what the row says or where it lives.
    pub fn matches_search(&self, index: usize) -> bool {
        let Some(control) = self.control(index) else {
            return false;
        };
        if control.kind == ControlKind::Section {
            return false;
        }
        let mut haystack = format!("{}\n{}\n{}", control.label, control.help, control.key);
        if let Some(section) = self.section_of(index) {
            haystack.push('\n');
            haystack.push_str(&self.section_path(section));
        }
        let haystack = haystack.to_lowercase();
        self.search
            .to_lowercase()
            .split_whitespace()
            .all(|word| haystack.contains(word))
    }

    /// A control the search may draw in place. One fed by a command is
    /// not: a room list is read from the chat client itself, and a
    /// search that touched twelve sections would talk to every client
    /// at once. Those come back as a way into their section instead.
    pub fn drawable_in_search(control: &PaneControl) -> bool {
        match control.kind {
            ControlKind::Toggle
            | ControlKind::Choice
            | ControlKind::Text
            | ControlKind::Number
            | ControlKind::Decimal
            | ControlKind::Strings
            | ControlKind::Button => true,
            ControlKind::Suggest => control.command.trim().is_empty(),
            _ => false,
        }
    }

    /// Matches that can only be reached by opening their section, one
    /// entry per (section, control).
    pub fn search_jumps(&self) -> Vec<(usize, usize)> {
        if !self.searching() {
            return Vec::new();
        }
        self.ext
            .manifest
            .pane
            .iter()
            .enumerate()
            .filter(|(i, c)| !Self::drawable_in_search(c) && self.matches_search(*i))
            .filter_map(|(i, _)| self.section_of(i).map(|s| (s, i)))
            .collect()
    }

    /// Is this control on screen?
    ///
    /// A control belongs to the nearest [`ControlKind::Section`] above
    /// it. Controls declared *before* the first section belong to none
    /// and are always shown, which is also what makes a plug-in with no
    /// sections render everything. While searching, what is shown is
    /// the matches instead, from every section, advanced or not.
    pub fn is_visible(&self, index: usize) -> bool {
        let Some(control) = self.control(index) else {
            return false;
        };
        if self.searching() {
            return Self::drawable_in_search(control) && self.matches_search(index);
        }
        if control.advanced && !self.advanced {
            return false;
        }
        let Some(selected) = self.selected_section() else {
            return true;
        };
        if index == selected {
            return true;
        }
        if control.kind == ControlKind::Section {
            return false;
        }
        self.section_of(index).is_none_or(|i| i == selected)
    }

    /// How many rows of the section on screen the basic view is hiding.
    pub fn hidden_advanced(&self) -> usize {
        if self.advanced || self.searching() {
            return 0;
        }
        let selected = self.selected_section();
        self.ext
            .manifest
            .pane
            .iter()
            .enumerate()
            .filter(|(i, c)| {
                c.advanced && c.kind != ControlKind::Section && self.section_of(*i) == selected
            })
            .count()
    }

    /// Show one section.
    ///
    /// Also the moment to re-read the arrays: reaching a section is the
    /// user's own step, and it is where a change made in an editor
    /// since the window opened gets picked up.
    pub fn select_section(&mut self, index: usize) {
        self.section = Some(index);
        self.search.clear();
        self.reload_arrays();
    }
}
