//! The decided-word cache — what makes an LLM usable on the correction
//! path at all.
//!
//! A query takes 30 ms on a warm local model and seconds on a hosted
//! API; a correction has to happen in the pause between two words. So
//! the detector never waits by default: it answers from here, and a
//! miss queues the question so the *next* occurrence is decided.
//!
//! That trade works because the same person types the same few thousand
//! words over and over, so even a small cache reaches a high hit rate
//! within a session, and the cost is a one-off "no opinion" the first
//! time a word appears.
//!
//! **What is stored is a verdict, not text a human can read back.**
//! Keys are hashes of the candidate list, never the words, and nothing
//! is written to disk.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

/// A remembered answer: the index the model chose among the candidates
/// it was given, or `None` for "none of these".
pub type Decision = Option<usize>;

/// Fixed-capacity map from question-hash to decision, with a
/// second-chance eviction: once full, insertion clears the oldest
/// half. Cruder than a true LRU and deliberately so — it needs no
/// per-entry bookkeeping on the read path, which is the path that
/// runs while the user is mid-correction.
pub struct DecisionCache {
    entries: HashMap<u64, Decision>,
    order: Vec<u64>,
    capacity: usize,
}

impl DecisionCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::with_capacity(capacity.min(1024)),
            order: Vec::with_capacity(capacity.min(1024)),
            capacity,
        }
    }

    /// Hash a question into a cache key.
    ///
    /// The candidate list *is* the question — same renderings, same
    /// answer — so it alone determines the key. Hashing rather than
    /// storing means the cache holds no recoverable copy of what was
    /// typed.
    pub fn key(candidates: &[String]) -> u64 {
        let mut h = DefaultHasher::new();
        candidates.len().hash(&mut h);
        for c in candidates {
            c.hash(&mut h);
        }
        h.finish()
    }

    pub fn get(&self, key: u64) -> Option<Decision> {
        self.entries.get(&key).copied()
    }

    pub fn insert(&mut self, key: u64, decision: Decision) {
        if self.capacity == 0 {
            return;
        }
        // An existing key is an update, not a new occupant: it must
        // not take a second slot in `order`, or a word re-decided a
        // few times would evict everything around it.
        if let std::collections::hash_map::Entry::Occupied(mut e) = self.entries.entry(key) {
            e.insert(decision);
            return;
        }
        if self.entries.len() >= self.capacity {
            // Drop the oldest half in one pass rather than one entry
            // per insert: amortised, and it keeps `order` from needing
            // an O(n) remove on every write.
            let cut = self.order.len() / 2;
            for old in self.order.drain(..cut) {
                self.entries.remove(&old);
            }
        }
        self.entries.insert(key, decision);
        self.order.push(key);
    }

    /// Live entry count. Not used on any hot path — it exists so the
    /// eviction policy can be asserted rather than assumed.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests;
