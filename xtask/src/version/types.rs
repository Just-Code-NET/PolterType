//! Parsed semver data.

/// Versioned identifier as we use them: `MAJOR.MINOR.PATCH` plus an
/// optional `-<word>.<counter>` suffix.
///
/// A deliberate **subset** of SemVer — no multiple suffix components,
/// no build metadata, no non-numeric counters. Rejecting weirder shapes
/// catches typos a permissive parser would accept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Version {
    pub(crate) major: u64,
    pub(crate) minor: u64,
    pub(crate) patch: u64,
    pub(crate) pre: Option<PreRelease>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreRelease {
    /// `alpha`, `beta`, `rc`, …
    pub(crate) word: String,
    pub(crate) counter: u64,
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(p) = &self.pre {
            write!(f, "-{}.{}", p.word, p.counter)?;
        }
        Ok(())
    }
}
