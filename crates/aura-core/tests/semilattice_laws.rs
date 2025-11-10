//! Property tests for semilattice laws
//!
//! These tests verify that our CRDT implementations satisfy the algebraic laws
//! required for correctness and convergence.

use aura_core::semilattice::{Bottom, JoinSemilattice, MeetSemiLattice, Top};
use std::collections::{BTreeMap, BTreeSet};

// Test implementations for JoinSemilattice laws

#[test]
fn test_u64_join_associativity() {
    let a: u64 = 10;
    let b: u64 = 20;
    let c: u64 = 15;

    // (a ⊔ b) ⊔ c = a ⊔ (b ⊔ c)
    assert_eq!(a.join(&b).join(&c), a.join(&b.join(&c)));
}

#[test]
fn test_u64_join_commutativity() {
    let a: u64 = 10;
    let b: u64 = 20;

    // a ⊔ b = b ⊔ a
    assert_eq!(a.join(&b), b.join(&a));
}

#[test]
fn test_u64_join_idempotence() {
    let a: u64 = 10;

    // a ⊔ a = a
    assert_eq!(a.join(&a), a);
}

#[test]
fn test_u64_bottom_identity() {
    let a: u64 = 10;
    let bottom = u64::bottom();

    // a ⊔ ⊥ = a
    assert_eq!(a.join(&bottom), a);
    assert_eq!(bottom.join(&a), a);
}

// Test implementations for MeetSemiLattice laws

#[test]
fn test_u64_meet_associativity() {
    let a: u64 = 10;
    let b: u64 = 20;
    let c: u64 = 15;

    // (a ⊓ b) ⊓ c = a ⊓ (b ⊓ c)
    assert_eq!(a.meet(&b).meet(&c), a.meet(&b.meet(&c)));
}

#[test]
fn test_u64_meet_commutativity() {
    let a: u64 = 10;
    let b: u64 = 20;

    // a ⊓ b = b ⊓ a
    assert_eq!(a.meet(&b), b.meet(&a));
}

#[test]
fn test_u64_meet_idempotence() {
    let a: u64 = 10;

    // a ⊓ a = a
    assert_eq!(a.meet(&a), a);
}

#[test]
fn test_u64_top_identity() {
    let a: u64 = 10;
    let top = u64::top();

    // a ⊓ ⊤ = a
    assert_eq!(a.meet(&top), a);
    assert_eq!(top.meet(&a), a);
}

// Test vector join semilattice

#[test]
fn test_vec_join_laws() {
    let a = vec![1, 3, 5];
    let b = vec![2, 3, 4];
    let c = vec![1, 6];

    // Associativity: (a ⊔ b) ⊔ c = a ⊔ (b ⊔ c)
    assert_eq!(a.join(&b).join(&c), a.join(&b.join(&c)));

    // Commutativity: a ⊔ b = b ⊔ a
    assert_eq!(a.join(&b), b.join(&a));

    // Idempotence: a ⊔ a = a
    assert_eq!(a.join(&a), a);

    // Bottom identity: a ⊔ ⊥ = a
    let bottom = Vec::<i32>::bottom();
    assert_eq!(a.join(&bottom), a);
}

// Test BTreeMap join semilattice

#[test]
fn test_btreemap_join_laws() {
    let mut a = BTreeMap::new();
    a.insert("key1".to_string(), 10u64);
    a.insert("key2".to_string(), 20u64);

    let mut b = BTreeMap::new();
    b.insert("key2".to_string(), 15u64);
    b.insert("key3".to_string(), 30u64);

    let mut c = BTreeMap::new();
    c.insert("key1".to_string(), 5u64);

    // Associativity: (a ⊔ b) ⊔ c = a ⊔ (b ⊔ c)
    assert_eq!(a.join(&b).join(&c), a.join(&b.join(&c)));

    // Commutativity: a ⊔ b = b ⊔ a
    assert_eq!(a.join(&b), b.join(&a));

    // Idempotence: a ⊔ a = a
    assert_eq!(a.join(&a), a);

    // Bottom identity: a ⊔ ⊥ = a
    let bottom = BTreeMap::<String, u64>::bottom();
    assert_eq!(a.join(&bottom), a);
}

// Test BTreeSet meet semilattice

#[test]
fn test_btreeset_meet_laws() {
    let a: BTreeSet<i32> = [1, 2, 3, 4].iter().cloned().collect();
    let b: BTreeSet<i32> = [3, 4, 5, 6].iter().cloned().collect();
    let c: BTreeSet<i32> = [2, 3, 7].iter().cloned().collect();

    // Associativity: (a ⊓ b) ⊓ c = a ⊓ (b ⊓ c)
    assert_eq!(a.meet(&b).meet(&c), a.meet(&b.meet(&c)));

    // Commutativity: a ⊓ b = b ⊓ a
    assert_eq!(a.meet(&b), b.meet(&a));

    // Idempotence: a ⊓ a = a
    assert_eq!(a.meet(&a), a);
}

// Test that join and meet are order-consistent

#[test]
fn test_join_meet_consistency() {
    // For u64: join should be max, meet should be min
    let a: u64 = 10;
    let b: u64 = 20;

    assert_eq!(a.join(&b), 20); // max
    assert_eq!(a.meet(&b), 10); // min

    // For sets: join should be union (larger), meet should be intersection (smaller)
    let set1: BTreeSet<i32> = [1, 2, 3].iter().cloned().collect();
    let set2: BTreeSet<i32> = [2, 3, 4].iter().cloned().collect();

    let expected_intersection: BTreeSet<i32> = [2, 3].iter().cloned().collect();
    assert_eq!(set1.meet(&set2), expected_intersection);
}
