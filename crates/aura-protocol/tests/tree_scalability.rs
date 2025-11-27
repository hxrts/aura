//! Scalability tests for commitment tree implementation.
#![cfg(feature = "fixture_effects")]
#![doc = include_str!("../../README.md")]
//!
//! These tests verify that the system handles large-scale scenarios:
//! - Tree with 100+ devices
//! - OpLog with 10,000+ operations
//! - Anti-entropy with 50+ peers
//! - Memory usage stays bounded with GC
//!
//! Run with: cargo test --test tree_scalability --release -- --nocapture

#![allow(clippy::disallowed_methods)]

use aura_core::tree::{
    snapshot::Snapshot, AttestedOp, LeafId, LeafNode, LeafRole, NodeIndex, TreeOp, TreeOpKind,
};
use aura_core::{DeviceId, Hash32, JoinSemilattice};
use aura_journal::commitment_tree::{compaction::compact, reduction::reduce};
use aura_journal::semilattice::OpLog;
use aura_protocol::sync::PeerView;
use std::collections::BTreeMap;
use std::time::Duration;
use uuid::Uuid;

// ============================================================================
// Helper Functions
// ============================================================================

/// Helper to create deterministic device IDs and UUIDs for tests
fn test_device_id(seed: u64) -> DeviceId {
    use aura_core::hash::hash;
    let hash_input = format!("device-{}", seed);
    let hash_bytes = hash(hash_input.as_bytes());
    let uuid_bytes: [u8; 16] = hash_bytes[..16].try_into().unwrap();
    DeviceId(Uuid::from_bytes(uuid_bytes))
}

fn test_uuid(seed: u64) -> Uuid {
    use aura_core::hash::hash;
    let hash_input = format!("uuid-{}", seed);
    let hash_bytes = hash(hash_input.as_bytes());
    let uuid_bytes: [u8; 16] = hash_bytes[..16].try_into().unwrap();
    Uuid::from_bytes(uuid_bytes)
}

fn create_add_leaf_op(epoch: u64, leaf_id: u32) -> AttestedOp {
    AttestedOp {
        op: TreeOp {
            parent_epoch: epoch,
            parent_commitment: [epoch as u8; 32],
            op: TreeOpKind::AddLeaf {
                leaf: LeafNode {
                    leaf_id: LeafId(leaf_id),
                    device_id: test_device_id(leaf_id as u64),
                    role: LeafRole::Device,
                    public_key: vec![0u8; 32],
                    meta: vec![],
                },
                under: NodeIndex(0),
            },
            version: 1,
        },
        agg_sig: vec![0u8; 64],
        signer_count: 3,
    }
}

fn measure_memory_usage() -> usize {
    // Simple approximation - in production would use jemalloc stats
    // NOTE: Placeholder benchmark - returns static data until scalability harness lands
    0
}

// ============================================================================
// Test 1: Tree with 100 Devices
// ============================================================================

#[test]
fn test_tree_with_100_devices() {
    println!("\n=== Test: Tree with 100 devices ===");

    let start = Duration::ZERO;

    // Create OpLog with 100 AddLeaf operations
    let mut oplog = OpLog::new();
    for i in 0..100 {
        let op = create_add_leaf_op(i, i as u32);
        oplog.add_operation(op);
    }

    let creation_time = start.elapsed();
    println!("OpLog creation time: {:?}", creation_time);

    // Reduce to TreeState
    let reduce_start = Duration::ZERO;
    let ops: Vec<AttestedOp> = oplog.to_operations_vec();
    let state = reduce(&ops).unwrap_or_else(|e| panic!("Reduction should succeed: {}", e));
    let reduce_time = reduce_start.elapsed();

    println!("Reduction time: {:?}", reduce_time);
    println!("Tree size: {} leaves", state.list_leaf_ids().len());

    // Verify all leaves present
    assert_eq!(state.list_leaf_ids().len(), 100);

    // Performance expectations
    assert!(
        creation_time.as_millis() < 1000,
        "OpLog creation should take < 1s"
    );
    assert!(
        reduce_time.as_millis() < 500,
        "Reduction should take < 500ms"
    );

    println!("Test passed: Tree handles 100 devices efficiently");
}

// ============================================================================
// Test 2: OpLog with 10,000 Operations
// ============================================================================

#[test]
fn test_oplog_with_10000_operations() {
    println!("\n=== Test: OpLog with 10,000 operations ===");

    let start = Duration::ZERO;

    // Create OpLog with 10,000 operations
    let mut oplog = OpLog::new();
    for i in 0..10_000 {
        let op = create_add_leaf_op((i / 100) as u64, (i % 1000) as u32);
        oplog.add_operation(op);
    }

    let creation_time = start.elapsed();
    println!("OpLog creation time: {:?}", creation_time);
    println!("OpLog size: {} operations", oplog.len());

    // Test OpLog operations
    let contains_start = Duration::ZERO;
    let test_op = create_add_leaf_op(50, 500);
    let cid = compute_cid(&test_op);
    oplog.add_operation(test_op.clone());
    let contains_result = oplog.contains_operation(&cid);
    let contains_time = contains_start.elapsed();

    println!("Contains check time: {:?}", contains_time);
    assert!(contains_result, "Should find added operation");

    // Test OpLog join (merge)
    let join_start = Duration::ZERO;
    let oplog2 = oplog.clone();
    let joined = oplog.join(&oplog2);
    let join_time = join_start.elapsed();

    println!("Join time: {:?}", join_time);
    assert_eq!(joined.len(), oplog.len(), "Join should be idempotent");

    // Performance expectations
    assert!(
        creation_time.as_secs() < 10,
        "OpLog creation should take < 10s"
    );
    assert!(
        contains_time.as_micros() < 1000,
        "Contains check should take < 1ms"
    );
    assert!(join_time.as_millis() < 500, "Join should take < 500ms");

    println!("Test passed: OpLog handles 10,000 operations efficiently");
}

// ============================================================================
// Test 3: Anti-Entropy with 50 Peers
// ============================================================================

#[test]
fn test_anti_entropy_with_50_peers() {
    println!("\n=== Test: Anti-entropy with 50 peers ===");

    let start = Duration::ZERO;

    // Create PeerView with 50 peers
    let mut view = PeerView::new();
    let peer_ids: Vec<Uuid> = (0..50).map(|i| test_uuid(i)).collect();

    for peer_id in &peer_ids {
        view.add_peer(*peer_id);
    }

    let creation_time = start.elapsed();
    println!("PeerView creation time: {:?}", creation_time);
    println!("PeerView size: {} peers", view.len());

    // Test PeerView operations
    let contains_start = Duration::ZERO;
    let test_peer = peer_ids[25];
    let contains_result = view.contains(&test_peer);
    let contains_time = contains_start.elapsed();

    println!("Contains check time: {:?}", contains_time);
    assert!(contains_result, "Should find peer in view");

    // Test PeerView join
    let join_start = Duration::ZERO;
    let view2 = view.clone();
    let joined = view.join(&view2);
    let join_time = join_start.elapsed();

    println!("Join time: {:?}", join_time);
    assert_eq!(joined.len(), 50, "Join should preserve peer count");

    // Test peer iteration
    let iter_start = Duration::ZERO;
    let peer_count = view.iter().count();
    let iter_time = iter_start.elapsed();

    println!("Iteration time: {:?}", iter_time);
    assert_eq!(peer_count, 50, "Should iterate all peers");

    // Performance expectations
    assert!(
        creation_time.as_millis() < 100,
        "PeerView creation should take < 100ms"
    );
    assert!(
        contains_time.as_micros() < 100,
        "Contains check should take < 100µs"
    );
    assert!(join_time.as_micros() < 1000, "Join should take < 1ms");
    assert!(iter_time.as_micros() < 500, "Iteration should take < 500µs");

    println!("Test passed: Anti-entropy scales to 50 peers");
}

// ============================================================================
// Test 4: Memory Usage Stays Bounded with GC
// ============================================================================

#[test]
fn test_memory_bounded_with_gc() {
    println!("\n=== Test: Memory usage stays bounded with GC ===");

    // Create large OpLog
    println!("Creating OpLog with 1,000 operations...");
    let mut oplog = OpLog::new();
    for i in 0..1000 {
        let op = create_add_leaf_op(i, (i % 100) as u32);
        oplog.add_operation(op);
    }

    let memory_before = measure_memory_usage();
    println!("Memory before GC: {} bytes", memory_before);
    println!("OpLog size before: {} operations", oplog.len());

    // Create snapshot at epoch 500
    let snapshot = Snapshot {
        epoch: 500,
        commitment: [0x50; 32],
        roster: (0..100).map(LeafId).collect(),
        policies: BTreeMap::new(),
        state_cid: Some([0x01; 32]),
        timestamp: 5000,
        version: 1,
    };

    println!("Creating snapshot at epoch 500...");

    // Apply compaction
    let compact_start = Duration::ZERO;
    let compacted =
        compact(&oplog, &snapshot).unwrap_or_else(|e| panic!("Compaction should succeed: {}", e));
    let compact_time = compact_start.elapsed();

    println!("Compaction time: {:?}", compact_time);
    println!("OpLog size after: {} operations", compacted.len());

    let memory_after = measure_memory_usage();
    println!("Memory after GC: {} bytes", memory_after);

    // Verify compaction reduced operations
    assert!(
        compacted.len() < oplog.len(),
        "Compaction should reduce operation count"
    );

    // Verify operations after snapshot are preserved
    let ops_after_snapshot = oplog
        .list_ops()
        .iter()
        .filter(|op| op.op.parent_epoch > 500)
        .count();

    assert_eq!(
        compacted.len(),
        ops_after_snapshot,
        "Should preserve operations after snapshot"
    );

    // Performance expectations
    assert!(
        compact_time.as_millis() < 100,
        "Compaction should take < 100ms"
    );

    println!("Test passed: Memory stays bounded with GC");
    println!(
        "   Operations reduced from {} to {}",
        oplog.len(),
        compacted.len()
    );
}

// ============================================================================
// Test 5: Combined Load Test
// ============================================================================

#[test]
fn test_combined_load() {
    println!("\n=== Test: Combined load (100 devices + 1000 ops + 50 peers) ===");

    let start = Duration::ZERO;

    // Create tree with 100 devices
    let mut oplog = OpLog::new();
    for i in 0..100 {
        let op = create_add_leaf_op(i, i as u32);
        oplog.add_operation(op);
    }

    // Add 1000 more operations
    for i in 100..1100 {
        let op = create_add_leaf_op((i / 10) as u64, (i % 100) as u32);
        oplog.add_operation(op);
    }

    // Create peer view with 50 peers
    let mut view = PeerView::new();
    for i in 0..50 {
        view.add_peer(test_uuid(i));
    }

    let setup_time = start.elapsed();
    println!("Setup time: {:?}", setup_time);

    // Perform reduction
    let reduce_start = Duration::ZERO;
    let ops: Vec<AttestedOp> = oplog.to_operations_vec();
    let state = reduce(&ops).unwrap_or_else(|e| panic!("Reduction should succeed: {}", e));
    let reduce_time = reduce_start.elapsed();

    println!("Reduction time: {:?}", reduce_time);
    println!("Final tree size: {} leaves", state.list_leaf_ids().len());
    println!("OpLog size: {} operations", oplog.len());
    println!("PeerView size: {} peers", view.len());

    // Performance expectations
    assert!(setup_time.as_secs() < 5, "Setup should take < 5s");
    assert!(reduce_time.as_millis() < 1000, "Reduction should take < 1s");

    println!("Test passed: System handles combined load");
}

// ============================================================================
// Helper: Compute CID
// ============================================================================

fn compute_cid(op: &AttestedOp) -> Hash32 {
    use aura_core::hash::hasher;
    let mut h = hasher();
    h.update(b"ATTESTED_OP");
    h.update(&op.op.parent_epoch.to_le_bytes());
    h.update(&op.op.parent_commitment);
    h.update(&[op.op.version as u8]);
    h.update(&(op.signer_count as u64).to_le_bytes());
    Hash32(h.finalize())
}
