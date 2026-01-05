//! Performance tests for consistency metadata types.
//!
//! These tests validate that consistency metadata operations perform
//! acceptably at scale. They are marked #[ignore] by default as they
//! take longer to run and are intended for benchmarking rather than CI.
//!
//! Run with: cargo test -p aura-core --test consistency_performance -- --ignored

#![allow(clippy::expect_used, clippy::disallowed_methods, missing_docs)]

use aura_core::{
    domain::{
        acknowledgment::Acknowledgment,
        agreement::Agreement,
        consistency::{Consistency, ConsistencyMap},
        propagation::Propagation,
    },
    query::ConsensusId,
    time::PhysicalTime,
    types::AuthorityId,
};
use std::time::Instant;
use uuid::Uuid;

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn test_authority(n: u16) -> AuthorityId {
    let bytes = n.to_be_bytes();
    let mut arr = [0u8; 16];
    arr[14] = bytes[0];
    arr[15] = bytes[1];
    AuthorityId::from_uuid(Uuid::from_bytes(arr))
}

fn test_time(millis: u64) -> PhysicalTime {
    PhysicalTime {
        ts_ms: millis,
        uncertainty: None,
    }
}

fn create_acknowledgment_for_peers(num_peers: u16) -> Acknowledgment {
    let mut ack = Acknowledgment::new();
    for i in 0..num_peers {
        ack.record_ack(test_authority(i), test_time(1000 + i as u64));
    }
    ack
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Ack Storage at Scale (9.4.1)
// ─────────────────────────────────────────────────────────────────────────────

/// Test acknowledgment storage with 10k messages and 100 peers each.
///
/// This simulates a high-volume chat channel where each message needs
/// to track acknowledgments from all 100 channel members.
///
/// Target: < 100ms for construction, < 10ms for lookups
#[test]
#[ignore]
fn test_ack_storage_at_scale() {
    const NUM_MESSAGES: usize = 10_000;
    const NUM_PEERS: u16 = 100;

    println!("\n=== Ack Storage at Scale Test ===");
    println!("Messages: {NUM_MESSAGES}, Peers per message: {NUM_PEERS}");

    // Phase 1: Create acknowledgments for all messages
    let start = Instant::now();
    let mut all_acks: Vec<Acknowledgment> = Vec::with_capacity(NUM_MESSAGES);

    for _ in 0..NUM_MESSAGES {
        let ack = create_acknowledgment_for_peers(NUM_PEERS);
        all_acks.push(ack);
    }

    let creation_time = start.elapsed();
    println!(
        "Created {NUM_MESSAGES} acknowledgments with {NUM_PEERS} peers each in {creation_time:?}"
    );

    // Verify structure
    assert_eq!(all_acks.len(), NUM_MESSAGES);
    assert_eq!(all_acks[0].count(), NUM_PEERS as usize);

    // Phase 2: Test lookup performance
    let start = Instant::now();
    let target_peer = test_authority(50);
    let mut found_count = 0;

    for ack in &all_acks {
        if ack.contains(&target_peer) {
            found_count += 1;
        }
    }

    let lookup_time = start.elapsed();
    println!("Looked up peer 50 in all messages in {lookup_time:?}");
    assert_eq!(found_count, NUM_MESSAGES);

    // Phase 3: Test all_acked performance
    let start = Instant::now();
    let expected_peers: Vec<AuthorityId> = (0..NUM_PEERS).map(test_authority).collect();
    let mut all_acked_count = 0;

    for ack in &all_acks {
        if ack.all_acked(&expected_peers) {
            all_acked_count += 1;
        }
    }

    let all_acked_time = start.elapsed();
    println!("Checked all_acked for {NUM_MESSAGES} messages in {all_acked_time:?}");
    assert_eq!(all_acked_count, NUM_MESSAGES);

    // Phase 4: Test merge performance (CRDT join)
    let start = Instant::now();
    let ack1 = create_acknowledgment_for_peers(50);
    let ack2 = create_acknowledgment_for_peers(100); // Overlapping 50 peers

    for _ in 0..1000 {
        let _ = ack1.clone().merge(&ack2);
    }

    let merge_time = start.elapsed();
    println!("Merged acknowledgments 1000 times in {merge_time:?}");

    // Report summary
    println!("\n=== Summary ===");
    println!(
        "Creation: {:?} ({:.2} μs/message)",
        creation_time,
        creation_time.as_micros() as f64 / NUM_MESSAGES as f64
    );
    println!(
        "Lookup: {:?} ({:.2} ns/message)",
        lookup_time,
        lookup_time.as_nanos() as f64 / NUM_MESSAGES as f64
    );
    println!(
        "all_acked: {:?} ({:.2} μs/message)",
        all_acked_time,
        all_acked_time.as_micros() as f64 / NUM_MESSAGES as f64
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: ConsistencyMap Query Performance (9.4.2)
// ─────────────────────────────────────────────────────────────────────────────

/// Test ConsistencyMap with 10k entries.
///
/// ConsistencyMap is used in query results to provide consistency metadata
/// for each returned item. This tests lookup performance at scale.
///
/// Target: < 100ms for construction, < 1ms for 1000 lookups
#[test]
#[ignore]
fn test_consistency_map_query_performance() {
    const NUM_ENTRIES: usize = 10_000;
    const NUM_LOOKUPS: usize = 1_000;

    println!("\n=== ConsistencyMap Query Performance Test ===");
    println!("Entries: {NUM_ENTRIES}, Lookups: {NUM_LOOKUPS}");

    // Phase 1: Build large ConsistencyMap
    let start = Instant::now();
    let mut map = ConsistencyMap::new();

    for i in 0..NUM_ENTRIES {
        let id = format!("item-{i:05}");

        // Vary the consistency state
        let consistency = if i % 3 == 0 {
            // Finalized
            Consistency::optimistic()
                .with_agreement(Agreement::finalized(ConsensusId::new([i as u8; 32])))
                .with_propagation(Propagation::complete())
        } else if i % 3 == 1 {
            // Syncing with acks
            let ack = create_acknowledgment_for_peers((i % 50) as u16 + 1);
            Consistency::optimistic()
                .with_propagation(Propagation::syncing((i % 10) as u16, 10))
                .with_acknowledgment(ack)
        } else {
            // Local
            Consistency::optimistic()
        };

        map.insert(id, consistency);
    }

    let build_time = start.elapsed();
    println!("Built ConsistencyMap with {NUM_ENTRIES} entries in {build_time:?}");

    assert_eq!(map.len(), NUM_ENTRIES);

    // Phase 2: Random lookups
    let start = Instant::now();
    let mut finalized_count = 0;
    let mut safe_count = 0;

    for i in 0..NUM_LOOKUPS {
        let idx = (i * 7919) % NUM_ENTRIES; // Pseudo-random access pattern
        let id = format!("item-{idx:05}");

        if map.is_finalized(&id) {
            finalized_count += 1;
        }
        if map.is_safe(&id) {
            safe_count += 1;
        }
    }

    let total_lookups = NUM_LOOKUPS * 2;
    let lookup_time = start.elapsed();
    println!("Performed {total_lookups} lookups (is_finalized + is_safe) in {lookup_time:?}");
    println!("Found {finalized_count} finalized, {safe_count} safe");

    // Phase 3: Iterate all entries
    let start = Instant::now();
    let mut total_ack_count = 0;

    for (_, consistency) in map.iter() {
        total_ack_count += consistency.ack_count();
    }

    let iter_time = start.elapsed();
    println!("Iterated all {NUM_ENTRIES} entries in {iter_time:?}, total acks: {total_ack_count}");

    // Phase 4: Test merge performance
    let start = Instant::now();
    let mut map2 = ConsistencyMap::new();
    for i in NUM_ENTRIES..NUM_ENTRIES + 1000 {
        let id = format!("item-{i:05}");
        map2.insert(id, Consistency::optimistic());
    }

    map.merge(map2);
    let merge_time = start.elapsed();
    println!("Merged 1000 entries in {merge_time:?}");
    assert_eq!(map.len(), NUM_ENTRIES + 1000);

    // Report summary
    println!("\n=== Summary ===");
    println!(
        "Build: {:?} ({:.2} μs/entry)",
        build_time,
        build_time.as_micros() as f64 / NUM_ENTRIES as f64
    );
    println!(
        "Lookup: {:?} ({:.2} ns/lookup)",
        lookup_time,
        lookup_time.as_nanos() as f64 / (NUM_LOOKUPS * 2) as f64
    );
    println!(
        "Iterate: {:?} ({:.2} ns/entry)",
        iter_time,
        iter_time.as_nanos() as f64 / NUM_ENTRIES as f64
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: GC Performance Under Load (9.4.3)
// ─────────────────────────────────────────────────────────────────────────────

/// Simulate garbage collection of acknowledgments under load.
///
/// This tests the pattern where we have many messages with acks, and we
/// need to determine which ones can be garbage collected based on policy.
///
/// Target: < 50ms to process 10k messages for GC eligibility
#[test]
#[ignore]
fn test_gc_performance_under_load() {
    const NUM_MESSAGES: usize = 10_000;
    const NUM_PEERS: u16 = 20;

    println!("\n=== GC Performance Under Load Test ===");
    println!("Messages: {NUM_MESSAGES}, Peers: {NUM_PEERS}");

    // Phase 1: Create consistency entries with varying states
    let start = Instant::now();
    let expected_peers: Vec<AuthorityId> = (0..NUM_PEERS).map(test_authority).collect();
    let mut entries: Vec<(String, Consistency)> = Vec::with_capacity(NUM_MESSAGES);

    for i in 0..NUM_MESSAGES {
        let id = format!("msg-{i:05}");

        // Create different scenarios:
        // - 30% finalized and fully acked (can GC)
        // - 30% finalized but not fully acked (cannot GC)
        // - 20% not finalized but fully acked (cannot GC)
        // - 20% neither finalized nor fully acked (cannot GC)
        let consistency = match i % 10 {
            0..=2 => {
                // Finalized and fully acked
                let ack = create_acknowledgment_for_peers(NUM_PEERS);
                Consistency::optimistic()
                    .with_agreement(Agreement::finalized(ConsensusId::new([i as u8; 32])))
                    .with_propagation(Propagation::complete())
                    .with_acknowledgment(ack)
            }
            3..=5 => {
                // Finalized but missing some acks
                let ack = create_acknowledgment_for_peers(NUM_PEERS / 2);
                Consistency::optimistic()
                    .with_agreement(Agreement::finalized(ConsensusId::new([i as u8; 32])))
                    .with_propagation(Propagation::complete())
                    .with_acknowledgment(ack)
            }
            6..=7 => {
                // Not finalized but fully acked
                let ack = create_acknowledgment_for_peers(NUM_PEERS);
                Consistency::optimistic()
                    .with_agreement(Agreement::Provisional)
                    .with_propagation(Propagation::complete())
                    .with_acknowledgment(ack)
            }
            _ => {
                // Neither
                Consistency::optimistic()
                    .with_agreement(Agreement::Provisional)
                    .with_propagation(Propagation::syncing(5, 10))
                    .with_ack_tracking()
            }
        };

        entries.push((id, consistency));
    }

    let setup_time = start.elapsed();
    println!("Created {NUM_MESSAGES} entries in {setup_time:?}");

    // Phase 2: Simulate GC eligibility check (DropWhenFinalizedAndFullyAcked policy)
    let start = Instant::now();
    let mut gc_eligible: Vec<&str> = Vec::new();

    for (id, consistency) in &entries {
        let is_finalized = consistency.is_finalized();
        let is_fully_acked = consistency
            .acknowledgment
            .as_ref()
            .map(|ack| ack.all_acked(&expected_peers))
            .unwrap_or(false);

        if is_finalized && is_fully_acked {
            gc_eligible.push(id);
        }
    }

    let gc_check_time = start.elapsed();
    println!("GC eligibility check for {NUM_MESSAGES} messages in {gc_check_time:?}");
    println!(
        "Found {} eligible for GC (expected ~30%)",
        gc_eligible.len()
    );

    // Verify expected ratio
    let gc_ratio = gc_eligible.len() as f64 / NUM_MESSAGES as f64;
    assert!(
        gc_ratio > 0.25 && gc_ratio < 0.35,
        "GC ratio should be ~30%"
    );

    // Phase 3: Simulate batch GC operation
    let start = Instant::now();
    let mut remaining: Vec<(String, Consistency)> = Vec::new();
    let mut gc_count = 0;

    for (id, consistency) in entries {
        let is_finalized = consistency.is_finalized();
        let is_fully_acked = consistency
            .acknowledgment
            .as_ref()
            .map(|ack| ack.all_acked(&expected_peers))
            .unwrap_or(false);

        if is_finalized && is_fully_acked {
            gc_count += 1;
            // In real GC, we'd delete from journal here
        } else {
            remaining.push((id, consistency));
        }
    }

    let gc_execute_time = start.elapsed();
    println!("Executed GC batch (removed {gc_count}) in {gc_execute_time:?}");
    assert_eq!(remaining.len(), NUM_MESSAGES - gc_count);

    // Phase 4: Test missing_acks performance (for partial delivery tracking)
    let start = Instant::now();
    let mut total_missing = 0;

    for (_, consistency) in &remaining {
        if let Some(ack) = &consistency.acknowledgment {
            total_missing += ack.missing_acks(&expected_peers).len();
        }
    }

    let missing_check_time = start.elapsed();
    println!(
        "Checked missing acks for {} entries in {:?}, total missing: {}",
        remaining.len(),
        missing_check_time,
        total_missing
    );

    // Report summary
    println!("\n=== Summary ===");
    println!(
        "Setup: {:?} ({:.2} μs/entry)",
        setup_time,
        setup_time.as_micros() as f64 / NUM_MESSAGES as f64
    );
    println!(
        "GC Check: {:?} ({:.2} μs/entry)",
        gc_check_time,
        gc_check_time.as_micros() as f64 / NUM_MESSAGES as f64
    );
    println!(
        "GC Execute: {:?} ({:.2} μs/entry)",
        gc_execute_time,
        gc_execute_time.as_micros() as f64 / NUM_MESSAGES as f64
    );
    println!(
        "Missing Check: {:?} ({:.2} μs/entry)",
        missing_check_time,
        missing_check_time.as_micros() as f64 / remaining.len() as f64
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Standard tests (not ignored, run in CI)
// ─────────────────────────────────────────────────────────────────────────────

/// Sanity check that types work correctly at small scale.
#[test]
fn test_consistency_types_sanity() {
    let peer1 = test_authority(1);
    let peer2 = test_authority(2);

    let ack = Acknowledgment::new()
        .add_ack(peer1, test_time(1000))
        .add_ack(peer2, test_time(2000));

    assert_eq!(ack.count(), 2);
    assert!(ack.contains(&peer1));
    assert!(ack.contains(&peer2));

    let mut map = ConsistencyMap::new();
    map.insert(
        "test",
        Consistency::optimistic()
            .with_agreement(Agreement::finalized(ConsensusId::new([1; 32])))
            .with_acknowledgment(ack),
    );

    assert!(map.is_finalized("test"));
    assert_eq!(map.get("test").unwrap().ack_count(), 2);
}
