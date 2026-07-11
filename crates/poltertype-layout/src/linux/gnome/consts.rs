//! The gsettings schema the switcher drives.

use super::*;
use crate::linux::shared::xkb_to_bcp47;
use crate::{LayoutError, LayoutId, LayoutSwitcher};
use std::process::Command;
use tracing::{debug, warn};

pub(crate) const SCHEMA: &str = "org.gnome.desktop.input-sources";
