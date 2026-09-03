//! The gsettings schema the switcher drives.

use super::*;
use crate::linux::shared::xkb_to_bcp47;
use crate::{LayoutError, LayoutId, LayoutSwitcher};
use std::process::Command;
use tracing::{debug, warn};

pub(crate) const SCHEMA: &str = "org.gnome.desktop.input-sources";

/// Desktops whose own daemon applies `org.gnome.desktop.input-sources`.
///
/// A positive list on purpose: the schema outlives the desktop that
/// wrote it (`dconf` is a file in the user's home), so a stand-down
/// list of desktops that *don't* read it kept growing and still missed
/// six that took this backend on a stale key and corrected nothing —
/// see `docs/DECISIONS.md`, 2026-08-27. `POLTERTYPE_LAYOUT_BACKEND=gnome`
/// overrides this for a desktop we have not heard of.
///
/// Cinnamon (#26) and MATE, which used to have a branch each here, are
/// covered by this rule: neither names itself GNOME.
pub(crate) const GNOME_FAMILY_NAMES: [&str; 6] = [
    "gnome",
    "gnome-classic",
    "gnome-flashback",
    "unity",
    "budgie",
    "pantheon",
];
