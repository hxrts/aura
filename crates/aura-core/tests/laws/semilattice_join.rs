//! Property tests for semilattice laws
//!
//! These tests verify that our CRDT implementations satisfy the algebraic laws
//! required for correctness and convergence.

use aura_core::domain::journal::{Fact, FactValue};
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
    a.insert(String::from("key1"), 10u64);
    a.insert(String::from("key2"), 20u64);

    let mut b = BTreeMap::new();
    b.insert(String::from("key2"), 15u64);
    b.insert(String::from("key3"), 30u64);

    let mut c = BTreeMap::new();
    c.insert(String::from("key1"), 5u64);

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

// ============================================================================
// Fact JoinSemilattice laws
//
// Fact is the core CRDT for journal state. If join is not associative or
// commutative, replicas that merge in different orders will diverge and
// the system will never converge.
// ============================================================================

fn fact_with_entry(key: &str, value: &str) -> Fact {
    Fact::with_value(key, FactValue::String(value.to_string()))
        .expect("fact construction should succeed")
}

/// join(a, join(b, c)) = join(join(a, b), c)
#[test]
fn fact_join_associativity() {
    let a = fact_with_entry("x", "alpha");
    let b = fact_with_entry("y", "beta");
    let c = fact_with_entry("z", "gamma");

    let left = a.join(&b).join(&c);
    let right = a.join(&b.join(&c));
    assert_eq!(left, right, "Fact join must be associative");
}

/// join(a, b) = join(b, a)
#[test]
fn fact_join_commutativity() {
    let a = fact_with_entry("x", "alpha");
    let b = fact_with_entry("y", "beta");

    assert_eq!(a.join(&b), b.join(&a), "Fact join must be commutative");
}

/// join(a, a) = a
#[test]
fn fact_join_idempotence() {
    let a = fact_with_entry("x", "alpha");
    assert_eq!(a.join(&a), a, "Fact join must be idempotent");
}

/// join(bottom, a) = a
#[test]
fn fact_join_bottom_identity() {
    let a = fact_with_entry("x", "alpha");
    let bottom = Fact::bottom();

    assert_eq!(bottom.join(&a), a, "bottom ⊔ a must equal a");
    assert_eq!(a.join(&bottom), a, "a ⊔ bottom must equal a");
}

// ============================================================================
// FactValue JoinSemilattice laws
//
// FactValue is the per-key CRDT value type. Same convergence requirements.
// ============================================================================

/// join(a, b) = join(b, a) for String variant (lexicographic max)
#[test]
fn fact_value_string_join_commutativity() {
    let a = FactValue::String("alpha".to_string());
    let b = FactValue::String("beta".to_string());

    assert_eq!(a.join(&b), b.join(&a), "String join must be commutative");
}

/// join(a, a) = a for Number variant
#[test]
fn fact_value_number_join_idempotence() {
    let a = FactValue::Number(42);
    assert_eq!(a.join(&a), a, "Number join must be idempotent");
}

/// join(a, b) = max(a, b) for Number variant
#[test]
fn fact_value_number_join_is_max() {
    let a = FactValue::Number(10);
    let b = FactValue::Number(20);

    assert_eq!(a.join(&b), FactValue::Number(20));
    assert_eq!(b.join(&a), FactValue::Number(20));
}

/// Set variant join is union (proper set-theoretic join)
#[test]
fn fact_value_set_join_is_union() {
    let a = FactValue::Set(["x".to_string(), "y".to_string()].into_iter().collect());
    let b = FactValue::Set(["y".to_string(), "z".to_string()].into_iter().collect());

    let joined = a.join(&b);
    if let FactValue::Set(values) = &joined {
        assert!(values.contains(&"x".to_string()));
        assert!(values.contains(&"y".to_string()));
        assert!(values.contains(&"z".to_string()));
    } else {
        panic!("Set join should produce Set");
    }

    // Commutativity
    let joined_rev = b.join(&a);
    if let FactValue::Set(values) = &joined_rev {
        assert!(values.contains(&"x".to_string()));
        assert!(values.contains(&"z".to_string()));
    } else {
        panic!("Set join should produce Set");
    }
}
