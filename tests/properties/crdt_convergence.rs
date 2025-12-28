//! CRDT Convergence Property Tests
//!
//! This test suite verifies that CRDT implementations in Aura satisfy
//! the fundamental convergence properties required for distributed consistency.

use aura_core::semilattice::{Bottom, JoinSemilattice};
use aura_journal::algebra::{GCounter, GSet, LwwRegister};

/// Test GCounter convergence - state-based CRDT
#[test]
fn test_gcounter_convergence() {
    let mut counter_a = GCounter::bottom();
    let mut counter_b = GCounter::bottom();
    let mut counter_c = GCounter::bottom();

    // Simulate concurrent increments on different replicas
    counter_a.increment("device_a".to_string(), 5);
    counter_b.increment("device_b".to_string(), 3);
    counter_c.increment("device_c".to_string(), 7);

    // Test commutativity: a ⊔ b = b ⊔ a
    let ab = counter_a.join(&counter_b);
    let ba = counter_b.join(&counter_a);
    assert_eq!(
        ab.value(),
        ba.value(),
        "Commutativity: order shouldn't matter"
    );

    // Test associativity: (a ⊔ b) ⊔ c = a ⊔ (b ⊔ c)
    let ab_c = ab.join(&counter_c);
    let bc = counter_b.join(&counter_c);
    let a_bc = counter_a.join(&bc);
    assert_eq!(
        ab_c.value(),
        a_bc.value(),
        "Associativity: grouping shouldn't matter"
    );

    // Test idempotence: a ⊔ a = a
    let a_a = counter_a.join(&counter_a);
    assert_eq!(
        counter_a.value(),
        a_a.value(),
        "Idempotence: joining with self"
    );

    // Test convergence: all permutations converge to same value
    assert_eq!(ab_c.value(), 15, "All increments should sum: 5+3+7=15");
}

/// Test GSet convergence - grow-only set
#[test]
fn test_gset_convergence() {
    let mut set_a = GSet::<String>::bottom();
    let mut set_b = GSet::<String>::bottom();
    let mut set_c = GSet::<String>::bottom();

    // Add different elements to different replicas
    set_a.add("apple".to_string());
    set_a.add("banana".to_string());

    set_b.add("cherry".to_string());
    set_b.add("apple".to_string()); // Duplicate

    set_c.add("date".to_string());

    // Test commutativity
    let ab = set_a.join(&set_b);
    let ba = set_b.join(&set_a);
    assert_eq!(
        ab.0.len(),
        ba.0.len(),
        "Set size should be same regardless of join order"
    );

    // Test associativity
    let ab_c = ab.join(&set_c);
    let bc = set_b.join(&set_c);
    let a_bc = set_a.join(&bc);
    assert_eq!(
        ab_c.0.len(),
        a_bc.0.len(),
        "Set size should be same regardless of grouping"
    );

    // Test idempotence
    let a_a = set_a.join(&set_a);
    assert_eq!(
        set_a.0.len(),
        a_a.0.len(),
        "Joining with self shouldn't change size"
    );

    // Test convergence: all replicas should have all unique elements
    assert_eq!(
        ab_c.0.len(),
        4,
        "Should have 4 unique elements: apple, banana, cherry, date"
    );
    assert!(ab_c.contains(&"apple".to_string()));
    assert!(ab_c.contains(&"banana".to_string()));
    assert!(ab_c.contains(&"cherry".to_string()));
    assert!(ab_c.contains(&"date".to_string()));
}

/// Test LwwRegister convergence - last-write-wins register
#[test]
fn test_lww_register_convergence() {
    let mut reg_a = LwwRegister::new();
    let mut reg_b = LwwRegister::new();
    let mut reg_c = LwwRegister::new();

    // Set values with different timestamps
    reg_a.set("value_a".to_string(), 100, "device_a".to_string());
    reg_b.set("value_b".to_string(), 200, "device_b".to_string());
    reg_c.set("value_c".to_string(), 300, "device_c".to_string());

    // Test commutativity: join always picks latest timestamp
    let ab = reg_a.join(&reg_b);
    let ba = reg_b.join(&reg_a);
    assert_eq!(ab.get(), ba.get(), "Join should be commutative");
    assert_eq!(
        ab.get(),
        Some(&"value_b".to_string()),
        "Should pick timestamp 200 > 100"
    );

    // Test associativity
    let ab_c = ab.join(&reg_c);
    let bc = reg_b.join(&reg_c);
    let a_bc = reg_a.join(&bc);
    assert_eq!(ab_c.get(), a_bc.get(), "Join should be associative");
    assert_eq!(
        ab_c.get(),
        Some(&"value_c".to_string()),
        "Should pick timestamp 300 > 200 > 100"
    );

    // Test idempotence
    let a_a = reg_a.join(&reg_a);
    assert_eq!(reg_a.get(), a_a.get(), "Joining with self");
}

/// Test monotonicity property for join semilattices
#[test]
fn test_join_monotonicity() {
    let mut counter = GCounter::bottom();

    // Initial value
    let v0 = counter.value();

    // Increment and check monotonicity
    counter.increment("device1".to_string(), 5);
    let v1 = counter.value();
    assert!(v1 >= v0, "Value should increase or stay same");

    counter.increment("device2".to_string(), 3);
    let v2 = counter.value();
    assert!(v2 >= v1, "Value should increase or stay same");

    // Joining should also maintain monotonicity
    let mut other = GCounter::bottom();
    other.increment("device3".to_string(), 10);

    let joined = counter.join(&other);
    let v3 = joined.value();
    assert!(v3 >= v2, "Join should not decrease value");
    assert!(v3 >= other.value(), "Join should not decrease value");
}

/// Test that concurrent operations converge
#[test]
fn test_concurrent_updates_convergence() {
    // Simulate 3 replicas making concurrent changes
    let mut replica_a = GCounter::bottom();
    let mut replica_b = GCounter::bottom();
    let mut replica_c = GCounter::bottom();

    // Each replica increments concurrently
    replica_a.increment("device_a".to_string(), 10);
    replica_b.increment("device_b".to_string(), 20);
    replica_c.increment("device_c".to_string(), 30);

    // Simulate gossip protocol: each replica receives updates from others
    // Round 1: A←B, B←C, C←A
    replica_a = replica_a.join(&replica_b);
    replica_b = replica_b.join(&replica_c);
    replica_c = replica_c.join(&replica_a);

    // Round 2: A←C, B←A, C←B
    replica_a = replica_a.join(&replica_c);
    replica_b = replica_b.join(&replica_a);
    replica_c = replica_c.join(&replica_b);

    // After sufficient rounds, all replicas should converge
    assert_eq!(replica_a.value(), 60, "Replica A should converge to 60");
    assert_eq!(replica_b.value(), 60, "Replica B should converge to 60");
    assert_eq!(replica_c.value(), 60, "Replica C should converge to 60");
}

/// Test network partition healing
#[test]
fn test_partition_healing() {
    // Partition: [A, B] | [C]
    let mut partition_1a = GSet::<String>::bottom();
    let mut partition_1b = GSet::<String>::bottom();
    let mut partition_2 = GSet::<String>::bottom();

    // Partition 1 makes changes
    partition_1a.add("x".to_string());
    partition_1b.add("y".to_string());
    partition_1a = partition_1a.join(&partition_1b);
    partition_1b = partition_1a.clone();

    // Partition 2 makes changes independently
    partition_2.add("z".to_string());

    // Partitions are separate
    assert_eq!(partition_1a.0.len(), 2);
    assert_eq!(partition_2.0.len(), 1);

    // Network heals: partitions merge
    let healed_a = partition_1a.join(&partition_2);
    let healed_b = partition_1b.join(&partition_2);
    let healed_c = partition_2.join(&partition_1a);

    // All replicas should converge after healing
    assert_eq!(
        healed_a.0.len(),
        3,
        "Should have all elements after healing"
    );
    assert_eq!(
        healed_b.0.len(),
        3,
        "Should have all elements after healing"
    );
    assert_eq!(
        healed_c.0.len(),
        3,
        "Should have all elements after healing"
    );
}

/// Test associativity property explicitly
#[test]
fn test_associativity_property() {
    let a = GCounter::bottom();
    let mut b = GCounter::bottom();
    let mut c = GCounter::bottom();

    b.increment("b".to_string(), 5);
    c.increment("c".to_string(), 3);

    // (a ⊔ b) ⊔ c
    let ab = a.join(&b);
    let ab_c = ab.join(&c);

    // a ⊔ (b ⊔ c)
    let bc = b.join(&c);
    let a_bc = a.join(&bc);

    assert_eq!(ab_c.value(), a_bc.value(), "Associativity must hold");
}

/// Test commutativity property explicitly
#[test]
fn test_commutativity_property() {
    let mut a = GCounter::bottom();
    let mut b = GCounter::bottom();

    a.increment("a".to_string(), 7);
    b.increment("b".to_string(), 11);

    // a ⊔ b
    let ab = a.join(&b);

    // b ⊔ a
    let ba = b.join(&a);

    assert_eq!(ab.value(), ba.value(), "Commutativity must hold");
}

/// Test idempotence property explicitly
#[test]
fn test_idempotence_property() {
    let mut a = GCounter::bottom();
    a.increment("a".to_string(), 42);

    // a ⊔ a
    let a_a = a.join(&a);

    assert_eq!(a.value(), a_a.value(), "Idempotence must hold: a ⊔ a = a");
}

/// Test bottom element property
#[test]
fn test_bottom_element() {
    let bottom = GCounter::bottom();
    let mut a = GCounter::bottom();
    a.increment("device".to_string(), 5);

    // bottom ⊔ a = a
    let bottom_a = bottom.join(&a);
    assert_eq!(a.value(), bottom_a.value(), "Bottom element identity");

    // a ⊔ bottom = a
    let a_bottom = a.join(&bottom);
    assert_eq!(
        a.value(),
        a_bottom.value(),
        "Bottom element identity (commuted)"
    );
}
