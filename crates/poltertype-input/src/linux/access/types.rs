//! What one scan of `/dev/input` found.

use std::os::unix::fs::MetadataExt;
use std::path::Path;

/// What one scan of `/dev/input` found. Counts rather than devices:
/// this is everything the failure message is built from, which is what
/// lets a unit test drive every branch of it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct ScanFacts {
    /// `None` when `/dev/input` itself could not be listed.
    pub(crate) nodes: Option<usize>,
    pub(crate) opened: usize,
    /// Of the opened ones, how many advertise `KEY_A`.
    pub(crate) keyboards: usize,
    /// Verbatim errno text of the first thing that refused us.
    pub(crate) first_error: Option<String>,
    /// The node that produced `first_error`, as the kernel presents it.
    pub(crate) sample: Option<NodeFacts>,
}

/// Ownership of one device node, in the numeric form `stat` reports.
/// Numeric on purpose: resolving names needs NSS, and the whole point
/// of printing this is that the naming layer may be what is lying.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NodeFacts {
    pub(crate) name: String,
    pub(crate) uid: u32,
    pub(crate) gid: u32,
    pub(crate) mode: u32,
}

impl NodeFacts {
    pub(crate) fn of(path: &Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        Some(Self {
            name: path.display().to_string(),
            uid: meta.uid(),
            gid: meta.gid(),
            mode: meta.mode() & 0o7777,
        })
    }
}

impl std::fmt::Display for NodeFacts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} is uid={} gid={} mode={:04o}",
            self.name, self.uid, self.gid, self.mode
        )
    }
}
