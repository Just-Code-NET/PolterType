//! Version comparison.

use semver::Version;

use crate::enums::UpdateError;

/// Is `candidate` a version we should move *to* from `current`?
///
/// Strict semver ordering, which gives us pre-release handling for
/// free: `0.4.0-rc.1 < 0.4.0`, so a user running an rc gets offered
/// the final, and a user on the final is never dragged back to an rc.
/// A tag that isn't valid semver is an error, not a "probably fine" —
/// we would rather skip an update than install something whose version
/// we cannot reason about.
pub fn is_newer(candidate: &str, current: &str) -> Result<bool, UpdateError> {
    let candidate_v = parse(candidate)?;
    let current_v = parse(current)?;
    Ok(candidate_v > current_v)
}

fn parse(v: &str) -> Result<Version, UpdateError> {
    // Tolerate a `v` prefix: the git tag carries one, and it is an easy
    // thing to leave in the manifest by hand.
    let trimmed = v.trim().trim_start_matches('v');
    Version::parse(trimmed).map_err(|e| UpdateError::BadVersion(v.to_owned(), e))
}
