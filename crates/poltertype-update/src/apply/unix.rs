//! Shell-quoting shared by the Linux and macOS installer scripts, and
//! the process-group detachment both spawn their installer with.

use std::path::Path;
#[cfg(unix)]
use std::process::Command;

/// Quote a string for a POSIX shell: wrap in single quotes, and end/
/// reopen the quoting around any literal `'`. Handles every byte a
/// path or a unit name can contain, which `"$VAR"`-style interpolation
/// does not.
pub(super) fn sh_quote_str(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

pub(super) fn sh_quote(path: &Path) -> String {
    sh_quote_str(&path.to_string_lossy())
}

/// Give the installer its own process group so a signal that reaches
/// ours does not reach it too. Not sufficient under systemd, where a
/// helper dies in the same instant it was waiting for — see
/// `docs/DECISIONS.md`, 2026-08-28, and the `linux` module doc.
#[cfg(unix)]
pub(super) fn detach(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    cmd.process_group(0);
}
