//! Small shared helpers for operations.

use std::collections::HashSet;
use std::hash::Hash;

/// Whether `candidate` is a permutation of `current`: same length, same set of
/// elements, and no duplicates in `candidate`. Used by the page- and entry-reorder
/// operations, whose element sets (page ids, entry codes) are unique.
pub(crate) fn is_permutation<T: Eq + Hash + Copy>(candidate: &[T], current: &[T]) -> bool {
    let candidate_set: HashSet<T> = candidate.iter().copied().collect();
    candidate.len() == current.len()
        && candidate_set.len() == candidate.len()
        && current.iter().all(|item| candidate_set.contains(item))
}
