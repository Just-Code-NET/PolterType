//! Buffer hygiene limits.

/// Longest run of consecutive boundary keys (spaces, dots, …) we
/// track for previous-word re-open. Backspacing across a longer run
/// than this abandons the stash instead of guessing.
pub(crate) const MAX_BOUNDARY_RUN: usize = 8;
