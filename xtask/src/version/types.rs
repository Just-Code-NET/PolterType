//! Parsed semver data.

/// Versioned identifier as we use them: `MAJOR.MINOR.PATCH` plus an
/// optional pre-release suffix `-<word>.<counter>`. This is a
/// **subset** of full SemVer — we don't accept multiple suffix
/// components (`-alpha.1.beta`), arbitrary build metadata
/// (`+build.42`), or non-numeric counters (`-rc-final`). That's
/// fine: every poltertype release we've ever cut fits the subset,
/// and rejecting weirder shapes catches typos that a permissive
/// parser would silently accept.
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
