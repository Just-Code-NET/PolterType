//! Constants for locating our own binary.

/// What Linux appends to the `/proc/self/exe` link target once the
/// binary behind it has been unlinked. `std::env::current_exe()` hands
/// that string back verbatim (rust-lang/rust#69343), so the path we get
/// is `/usr/bin/poltertype (deleted)` — a file that does not exist.
pub(super) const DELETED_SUFFIX: &str = " (deleted)";
