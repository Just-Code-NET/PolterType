//! The decided-word cache — what makes an LLM usable on the correction
//! path at all.
//!
//! A query takes 30 ms on a warm local model and seconds on a hosted
//! API, while a correction has to happen in the pause between two
//! words. So the detector never waits by default: it answers from here,
//! and a miss queues the question so the *next* occurrence is decided.
//! The trade works because one person retypes the same few thousand
//! words, so even a small cache gets hot within a session.
//!
//! **What is stored is a verdict, not text a human can read back.**
//! Keys are hashes of the candidate list, never the words, and nothing
//! is written to disk.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

/// A remembered answer: the index the model chose among the candidates
/// it was given, or `None` for "none of these".
pub type Decision = Option<usize>;

/// Fixed-capacity map from question-hash to decision. Once full,
/// insertion clears the oldest half — cruder than a true LRU on
/// purpose, since it needs no per-entry bookkeeping on the read path,
/// the one that runs while the user is mid-correction.
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

    /// Hash a question into a cache key. The candidate list *is* the
    /// question, so it alone determines the key; hashing rather than
    /// storing keeps any recoverable copy of what was typed out of the
    /// cache.
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
        // An existing key must not take a second slot in `order`, or a
        // word re-decided a few times would evict everything around it.
        if let std::collections::hash_map::Entry::Occupied(mut e) = self.entries.entry(key) {
            e.insert(decision);
            return;
        }
        if self.entries.len() >= self.capacity {
            // Oldest half in one pass, so `order` never needs an O(n)
            // remove on a write.
            let cut = self.order.len() / 2;
            for old in self.order.drain(..cut) {
                self.entries.remove(&old);
            }
        }
        self.entries.insert(key, decision);
        self.order.push(key);
    }

    /// Live entry count, so the eviction policy can be asserted rather
    /// than assumed. Not on any hot path.
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
