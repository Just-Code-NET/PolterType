//! Handing a staged artifact to the OS installer.
//!
//! Windows and macOS have the same problem: the thing being replaced
//! is the thing doing the replacing. An MSI cannot overwrite a running
//! `poltertype.exe`. So neither backend installs anything directly —
//! each writes a small script into the staging directory and spawns
//! it, and the script's first act is to wait for our PID to disappear.
//! The installer runs in the gap and relaunches us after.
//!
//! Linux does not, and must not: a helper spawned by an app systemd
//! started is killed the moment that app exits, which is the moment it
//! was waiting for. It swaps the AppImage in-process instead — see
//! [`linux`], which carries the whole story.
//!
//! Paths go into the script as *files* rather than command-line
//! arguments: they are user home directories, which routinely contain
//! spaces, apostrophes and non-ASCII — exactly the input that turns
//! nested shell quoting into a bug. A script on disk has one layer of
//! quoting instead of three, and can be read afterwards by a user
//! asking what it did to their machine.

// All three are compiled when testing, whatever the host is: the
// installer script is text, and text is the only part of an installer
// that can be checked without installing something. Their assertions
// used to run only on the platform they install for — a poor place to
// keep the tests for a bug that shipped three times, and for a macOS
// backend nobody in the project can run at all.
#[cfg(any(target_os = "linux", test))]
mod linux;
#[cfg(any(target_os = "macos", test))]
mod macos;
#[cfg(any(target_os = "windows", test))]
mod windows;

mod consts;
mod dispatch;
mod script;
#[cfg(any(unix, test))]
mod unix;

#[cfg(all(test, unix))]
mod tests_util;

pub use dispatch::apply;
