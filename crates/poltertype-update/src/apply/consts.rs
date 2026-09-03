//! Constants the installer scripts and the wait for their first line
//! share across all three OS backends.

use std::time::Duration;

/// What every installer script prints before it does anything that can
/// fail. Read back while waiting for the installer's first line.
pub(super) const HELLO: &str = "PolterType installer: started";

/// How long to give the installer to say it is alive. Generous: a cold
/// `powershell.exe` on a machine with an eager antivirus can take a
/// second or two to reach its first statement, and a false negative
/// here costs the user an update they asked for.
pub(super) const GREETING_TIMEOUT: Duration = Duration::from_secs(5);
pub(super) const GREETING_POLL: Duration = Duration::from_millis(50);
