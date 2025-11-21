//! Semilattice Law Verification Tests
//!
//! Tests to verify that CRDT implementations satisfy the mathematical laws
//! of join-semilattices and meet-semilattices.

use aura_core::journal::Cap;
use aura_core::semilattice::{Bottom, JoinSemilattice, MeetSemiLattice, Top};
use aura_journal::semilattice::{GCounter, GSet};

/// Test GCounter satisfies join semilattice laws
#[test]
fn test_gcounter_semilattice_laws() {
    let mut a = GCounter::bottom();
    let mut b = GCounter::bottom();
    let mut c = GCounter::bottom();

    a.increment("device_a".to_string(), 10);
    b.increment("device_b".to_string(), 20);
    c.increment("device_c".to_string(), 15);

    // Test associativity: (a ⊔ b) ⊔ c = a ⊔ (b ⊔ c)
    let ab_join_c = a.join(&b).join(&c);
    let a_join_bc = a.join(&b.join(&c));
    assert_eq!(
        ab_join_c.value(),
        a_join_bc.value(),
        "Associativity violated"
    );

    // Test commutativity: a ⊔ b = b ⊔ a
    let a_join_b = a.join(&b);
    let b_join_a = b.join(&a);
    assert_eq!(a_join_b.value(), b_join_a.value(), "Commutativity violated");

    // Test idempotency: a ⊔ a = a
    let a_join_a = a.join(&a);
    assert_eq!(a.value(), a_join_a.value(), "Idempotency violated");
}

/// Test GSet satisfies join semilattice laws
#[test]
fn test_gset_semilattice_laws() {
    let mut a = GSet::<String>::bottom();
    let mut b = GSet::<String>::bottom();
    let mut c = GSet::<String>::bottom();

    a.add("x".to_string());
    a.add("y".to_string());
    b.add("y".to_string());
    b.add("z".to_string());
    c.add("w".to_string());

    // Test associativity: (a ⊔ b) ⊔ c = a ⊔ (b ⊔ c)
    let ab_join_c = a.join(&b).join(&c);
    let a_join_bc = a.join(&b.join(&c));
    assert_eq!(
        ab_join_c.0.len(),
        a_join_bc.0.len(),
        "Associativity violated"
    );

    // Test commutativity: a ⊔ b = b ⊔ a
    let a_join_b = a.join(&b);
    let b_join_a = b.join(&a);
    assert_eq!(a_join_b.0.len(), b_join_a.0.len(), "Commutativity violated");

    // Test idempotency: a ⊔ a = a
    let a_join_a = a.join(&a);
    assert_eq!(a.0.len(), a_join_a.0.len(), "Idempotency violated");
}

/// Test Cap satisfies meet semilattice laws
#[test]
fn test_cap_meet_semilattice_laws() {
    let a = Cap::new();
    let b = Cap::new();
    let c = Cap::new();

    // Test associativity: (a ⊓ b) ⊓ c = a ⊓ (b ⊓ c)
    let ab_meet_c = a.meet(&b).meet(&c);
    let a_meet_bc = a.meet(&b.meet(&c));
    assert_eq!(ab_meet_c, a_meet_bc, "Meet associativity violated");

    // Test commutativity: a ⊓ b = b ⊓ a
    let a_meet_b = a.meet(&b);
    let b_meet_a = b.meet(&a);
    assert_eq!(a_meet_b, b_meet_a, "Meet commutativity violated");

    // Test idempotency: a ⊓ a = a
    let a_meet_a = a.meet(&a);
    assert_eq!(a, a_meet_a, "Meet idempotency violated");
}

/// Test bottom element for join semilattice
#[test]
fn test_bottom_element_laws() {
    let bottom = GCounter::bottom();
    let mut a = GCounter::bottom();
    a.increment("device".to_string(), 42);

    // bottom ⊔ a = a
    let bottom_join_a = bottom.join(&a);
    assert_eq!(a.value(), bottom_join_a.value(), "Bottom identity violated");

    // a ⊔ bottom = a
    let a_join_bottom = a.join(&bottom);
    assert_eq!(
        a.value(),
        a_join_bottom.value(),
        "Bottom identity violated (commuted)"
    );
}

/// Test top element for meet semilattice
#[test]
fn test_top_element_laws() {
    let top = Cap::top();
    let a = Cap::new();

    // top ⊓ a = a
    let top_meet_a = top.meet(&a);
    assert_eq!(a, top_meet_a, "Top identity violated");

    // a ⊓ top = a
    let a_meet_top = a.meet(&top);
    assert_eq!(a, a_meet_top, "Top identity violated (commuted)");
}

/// Test absorption law where applicable
#[test]
fn test_join_absorption() {
    let mut a = GCounter::bottom();
    a.increment("device".to_string(), 10);

    let b = a.clone();

    // a ⊔ a = a (special case of absorption)
    let a_join_a = a.join(&b);
    assert_eq!(a.value(), a_join_a.value(), "Self-join absorption violated");
}

/// Test monotonicity: a ≤ (a ⊔ b)
#[test]
fn test_join_monotonicity() {
    let mut a = GCounter::bottom();
    let mut b = GCounter::bottom();

    a.increment("device_a".to_string(), 10);
    b.increment("device_b".to_string(), 5);

    let a_join_b = a.join(&b);

    // Result should be at least as large as a
    assert!(a_join_b.value() >= a.value(), "Join monotonicity violated");
    // Result should be at least as large as b
    assert!(a_join_b.value() >= b.value(), "Join monotonicity violated");
}

/// Test all three semilattice laws together
#[test]
fn test_all_semilattice_laws_gcounter() {
    let mut a = GCounter::bottom();
    let mut b = GCounter::bottom();
    let mut c = GCounter::bottom();

    a.increment("a".to_string(), 5);
    b.increment("b".to_string(), 3);
    c.increment("c".to_string(), 7);

    // 1. Associativity
    let left = a.join(&b).join(&c);
    let right = a.join(&b.join(&c));
    assert_eq!(left.value(), right.value(), "Failed: Associativity");

    // 2. Commutativity
    let ab = a.join(&b);
    let ba = b.join(&a);
    assert_eq!(ab.value(), ba.value(), "Failed: Commutativity");

    // 3. Idempotency
    let aa = a.join(&a);
    assert_eq!(a.value(), aa.value(), "Failed: Idempotency");
}

/// Test that different join orders produce same result
#[test]
fn test_join_order_independence() {
    let mut a = GSet::<i32>::bottom();
    let mut b = GSet::<i32>::bottom();
    let mut c = GSet::<i32>::bottom();

    a.add(1);
    a.add(2);
    b.add(2);
    b.add(3);
    c.add(3);
    c.add(4);

    // Try different join orders
    let abc = a.join(&b).join(&c);
    let acb = a.join(&c).join(&b);
    let bac = b.join(&a).join(&c);
    let bca = b.join(&c).join(&a);
    let cab = c.join(&a).join(&b);
    let cba = c.join(&b).join(&a);

    // All should produce the same result
    assert_eq!(abc.0.len(), acb.0.len());
    assert_eq!(abc.0.len(), bac.0.len());
    assert_eq!(abc.0.len(), bca.0.len());
    assert_eq!(abc.0.len(), cab.0.len());
    assert_eq!(abc.0.len(), cba.0.len());
}
