//! Configuration for AI plug-ins.
//!
//! The schema itself lives in `poltertype-types` so that
//! `poltertype-core` can parse it without depending on this optional
//! crate; re-exported here because this is where callers look for it.

use crate::enums::{Locality, QueryMode, WireFormat};

pub use poltertype_types::AiPluginConfig;

/// Everything the detector needs, already validated by the factory.
pub struct LlmSettings {
    pub id: String,
    pub endpoint: String,
    pub format: WireFormat,
    pub model: String,
    pub api_key: Option<String>,
    /// A key was configured but the keychain could not supply it — the
    /// detector loads and stays silent rather than calling an endpoint
    /// that will certainly reject it.
    pub key_unavailable: bool,
    pub max_latency_ms: u64,
    pub mode: QueryMode,
    pub cache_size: usize,
    pub locality: Locality,
    /// `[ai].allow_remote`. Only consulted for a remote endpoint.
    pub allow_remote: bool,
}

impl LlmSettings {
    /// Whether this detector is allowed to make its call at all.
    pub fn permitted(&self) -> bool {
        cfg!(feature = "remote")
            && !self.key_unavailable
            && match self.locality {
                Locality::Loopback => true,
                Locality::Remote => self.allow_remote,
            }
    }
}
