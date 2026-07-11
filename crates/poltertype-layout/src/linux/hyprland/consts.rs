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
/// toggle — see `current()`.
pub(crate) const EMITTER_DEVICE_NAME: &str = "poltertype virtual keyboard";
