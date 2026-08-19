//! Version comparison.

use semver::Version;

use crate::enums::UpdateError;

/// Is `candidate` a version we should move *to* from `current`?
///
/// Strict semver ordering, which gives pre-release handling for free:
/// `0.4.0-rc.1 < 0.4.0`, so an rc user is offered the final and a final
/// user is never dragged back. A non-semver tag is an error rather than
/// "probably fine".
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
