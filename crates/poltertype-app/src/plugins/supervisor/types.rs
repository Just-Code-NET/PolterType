//! Plain data behind one supervised plug-in service.

use std::path::PathBuf;
use std::process::Child;

use poltertype_core::plugins::DiscoveredExtension;

/// One running service, and enough to identify it in a log line.
pub(super) struct Running {
    pub(super) id: String,
    pub(super) child: Child,
    /// The extension this process came from, kept so it can be asked to
    /// stop the way *it* declared. Cloning it costs a few strings once
    /// per plug-in at startup.
    pub(super) ext: DiscoveredExtension,
    /// Where this service's own output went, if we managed to open a
    /// file for it. Read only when the service is gone.
    pub(super) log: Option<PathBuf>,
}
