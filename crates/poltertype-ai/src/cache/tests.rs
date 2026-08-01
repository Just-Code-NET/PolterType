use super::*;

fn cands(words: &[&str]) -> Vec<String> {
    words.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn remembers_a_decision() {
    let mut c = DecisionCache::new(8);
    let k = DecisionCache::key(&cands(&["привіт", "ghbdsn"]));
    assert_eq!(c.get(k), None, "cold cache has no answer");
    c.insert(k, Some(0));
    assert_eq!(c.get(k), Some(Some(0)));
}

/// "None of these" is a real answer worth remembering — otherwise
/// every occurrence of a word the model rejected re-queries forever.
#[test]
fn a_negative_answer_is_cached_too() {
    let mut c = DecisionCache::new(8);
    let k = DecisionCache::key(&cands(&["qwerty"]));
    c.insert(k, None);
    assert_eq!(c.get(k), Some(None), "cached 'none of these'");
}

#[test]
fn the_candidate_list_determines_the_key() {
    let a = DecisionCache::key(&cands(&["слово", "ckjdj"]));
    let b = DecisionCache::key(&cands(&["слово", "ckjdj"]));
    assert_eq!(a, b, "same question, same key");

    let different = DecisionCache::key(&cands(&["ckjdj", "слово"]));
    assert_ne!(a, different, "order is part of the question");

    let shorter = DecisionCache::key(&cands(&["слово"]));
    assert_ne!(a, shorter, "candidate count is part of the question");
}

/// Length is hashed separately so that concatenation cannot collide:
/// ["ab","c"] and ["a","bc"] are different questions.
#[test]
fn concatenation_does_not_collide() {
    assert_ne!(
        DecisionCache::key(&cands(&["ab", "c"])),
        DecisionCache::key(&cands(&["a", "bc"]))
    );
}

#[test]
fn stays_within_capacity() {
    let mut c = DecisionCache::new(16);
    for i in 0..200 {
        c.insert(DecisionCache::key(&cands(&[&format!("w{i}")])), Some(0));
    }
    assert!(c.len() <= 16, "grew past capacity: {}", c.len());
    assert!(!c.is_empty(), "evicted everything");
}

#[test]
fn recent_entries_survive_eviction() {
    let mut c = DecisionCache::new(16);
    for i in 0..40 {
        c.insert(DecisionCache::key(&cands(&[&format!("w{i}")])), Some(0));
    }
    let newest = DecisionCache::key(&cands(&["w39"]));
    assert_eq!(c.get(newest), Some(Some(0)), "newest must still be there");
}

#[test]
fn re_inserting_a_key_updates_rather_than_grows() {
    let mut c = DecisionCache::new(4);
    let k = DecisionCache::key(&cands(&["x"]));
    c.insert(k, Some(0));
    c.insert(k, Some(1));
    assert_eq!(c.len(), 1, "same key must not occupy two slots");
    assert_eq!(c.get(k), Some(Some(1)), "later answer wins");
}

/// A zero capacity is a legal way to say "never remember anything".
#[test]
fn zero_capacity_stores_nothing() {
    let mut c = DecisionCache::new(0);
    let k = DecisionCache::key(&cands(&["x"]));
    c.insert(k, Some(0));
    assert_eq!(c.get(k), None);
    assert!(c.is_empty());
}
