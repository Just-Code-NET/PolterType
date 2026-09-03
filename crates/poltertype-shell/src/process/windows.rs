//! Windows: give a spawned child its own console but no window.

use std::os::windows::process::CommandExt as _;
use std::process::Command;

/// `CREATE_NO_WINDOW`, spelled out rather than pulled from
/// `windows-sys` — one stable ABI constant is cheaper than a Win32
/// binding crate. See `CreateProcess` → *Process Creation Flags*.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Prepare a `Command` the way a tray app has to spawn a child.
///
/// PolterType links as a GUI image and owns no console, so a console
/// child spawned from it gets one **allocated**, window and all — a
/// black window beside the tray for a daemon, and one flashing up on
/// every menu draw for the state query. `CREATE_NO_WINDOW` gives the
/// child its console without a window.
pub fn configure_child(cmd: &mut Command) {
    cmd.creation_flags(CREATE_NO_WINDOW);
}
