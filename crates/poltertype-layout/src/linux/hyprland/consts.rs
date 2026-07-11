//! Names the switcher must recognise / skip.

use super::*;
use crate::linux::shared::{cmd_exists, xkb_to_bcp47};
use crate::{LayoutError, LayoutId, LayoutSwitcher};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tracing::{debug, warn};

/// Name our uinput emitter registers itself under (see
/// `poltertype-input`'s `UinputEmitter`). We skip it when reading the active
/// layout because it never receives the user's manual Alt+Shift
/// toggle — see `current()`. NB: Hyprland normalises device names
/// (spaces → dashes), so always compare through
/// `normalize_device_name`, never with `==` on the raw strings.
pub(crate) const EMITTER_DEVICE_NAME: &str = "poltertype virtual keyboard";

/// Name fragments of input-remapper virtual keyboards. When such a
/// remapper is present, every physical keystroke reaches the
/// compositor through *its* device, and the user's per-device layout
/// toggle (`grp:*_toggle`) lands there too — so its keymap, not the
/// `main` device's, reflects what the user is actually typing in.
pub(crate) const REMAPPER_NAME_MARKERS: &[&str] = &["keyd", "kanata", "kmonad"];
