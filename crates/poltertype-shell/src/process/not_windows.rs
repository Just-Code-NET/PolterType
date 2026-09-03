//! Every other platform hands a spawned console program a window on
//! its own account already; there is nothing to configure.

use std::process::Command;

pub fn configure_child(_cmd: &mut Command) {}
