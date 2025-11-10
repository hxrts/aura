//! Performance benchmarks for synchronization protocols.
//!
//! Measures performance characteristics of:
//! - Anti-entropy sync time vs divergence
//! - Broadcast delivery latency
//! - PeerView CRDT operations
//! - IntentState merge operations
//!
//! Run with: cargo bench --bench sync_benchmarks

use aura_core::tree::{
    AttestedOp, Epoch, Hash32, LeafId, LeafNode, LeafRole, NodeIndex, TreeOp, TreeOpKind,
};
use aura_protocol::sync::{IntentState, PeerView};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use std::collections::BTreeMap;
use uuid::Uuid;

// ============================================================================
// Helper Functions
// ============================================================================

fn create_test_op(epoch: u64, leaf_id: u32) -> AttestedOp {
    AttestedOp {
        op: TreeOp {
            parent_epoch: Epoch(epoch),
            parent_commitment: [epoch as u8; 32],
            op: TreeOpKind::AddLeaf {
                leaf: LeafNode {
                    leaf_id: LeafId(leaf_id),
                    role: LeafRole::Device,
                    public_key: vec![0u8; 32],
                    meta: BTreeMap::new(),
                },
                under: NodeIndex(0),
            },
            version: 1,
        },
        agg_sig: vec![0u8; 64],
        signer_count: 3,
    }
}

fn create_peer_view_with_peers(num_peers: usize) -> PeerView {
    let mut view = PeerView::new();
    for _ in 0..num_peers {
        view.add_peer(Uuid::new_v4());
    }
    view
}

// ============================================================================
// Benchmark 1: PeerView CRDT Operations
// ============================================================================

fn bench_peer_view_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("peer_view_operations");

    // Benchmark add_peer
    group.bench_function("add_peer", |b| {
        let mut view = PeerView::new();
        let peer_id = Uuid::new_v4();

        b.iter(|| {
            view.add_peer(black_box(peer_id));
        });
    });

    // Benchmark join with different sizes
    for size in [10, 50, 100].iter() {
        let view1 = create_peer_view_with_peers(*size);
        let view2 = create_peer_view_with_peers(*size);

        group.bench_with_input(BenchmarkId::new("join", size), size, |b, _| {
            b.iter(|| {
                let joined = black_box(&view1).join(black_box(&view2));
                black_box(joined);
            });
        });
    }

    // Benchmark contains check
    let view = create_peer_view_with_peers(100);
    let peer_id = Uuid::new_v4();

    group.bench_function("contains", |b| {
        b.iter(|| {
            let result = view.contains(black_box(&peer_id));
            black_box(result);
        });
    });

    group.finish();
}

// ============================================================================
// Benchmark 2: IntentState Operations
// ============================================================================

fn bench_intent_state_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("intent_state_operations");

    // Benchmark state transitions
    let proposed = IntentState::Proposed { timestamp: 1000 };
    let attesting = IntentState::Attesting {
        timestamp: 1000,
        collected: 2,
    };
    let finalized = IntentState::Finalized { timestamp: 1000 };

    group.bench_function("merge_forward", |b| {
        b.iter(|| {
            let result = black_box(&proposed).merge(black_box(&finalized));
            black_box(result);
        });
    });

    group.bench_function("merge_reverse", |b| {
        b.iter(|| {
            let result = black_box(&finalized).merge(black_box(&proposed));
            black_box(result);
        });
    });

    group.bench_function("merge_concurrent", |b| {
        let aborted = IntentState::Aborted {
            timestamp: 2000,
            reason: 1,
        };

        b.iter(|| {
            let result = black_box(&aborted).merge(black_box(&finalized));
            black_box(result);
        });
    });

    // Benchmark state comparisons
    group.bench_function("partial_cmp", |b| {
        b.iter(|| {
            let result = black_box(&proposed).partial_cmp(black_box(&attesting));
            black_box(result);
        });
    });

    group.finish();
}

// ============================================================================
// Benchmark 3: Digest Computation (Bloom Filter Simulation)
// ============================================================================

fn bench_digest_computation(c: &mut Criterion) {
    use blake3::Hasher;

    let mut group = c.benchmark_group("digest_computation");

    // Simulate digest computation for different OpLog sizes
    for size in [10, 50, 100, 500, 1000].iter() {
        let ops: Vec<AttestedOp> = (0..*size)
            .map(|i| create_test_op(i as u64, i as u32))
            .collect();

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let mut hasher = Hasher::new();
                for op in &ops {
                    hasher.update(&op.op.parent_epoch.0.to_le_bytes());
                    hasher.update(&op.op.parent_commitment);
                }
                let digest = hasher.finalize();
                black_box(digest);
            });
        });
    }

    group.finish();
}

// ============================================================================
// Benchmark 4: Operation Difference Calculation
// ============================================================================

fn bench_difference_calculation(c: &mut Criterion) {
    use std::collections::BTreeSet;

    let mut group = c.benchmark_group("difference_calculation");

    for size in [10, 50, 100].iter() {
        let ops1: BTreeSet<u64> = (0..*size).collect();
        let ops2: BTreeSet<u64> = (size / 2..*size + size / 2).collect();

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let diff: Vec<_> = black_box(&ops1)
                    .difference(black_box(&ops2))
                    .copied()
                    .collect();
                black_box(diff);
            });
        });
    }

    group.finish();
}

// ============================================================================
// Benchmark 5: Message Serialization
// ============================================================================

fn bench_message_serialization(c: &mut Criterion) {
    use serde::{Deserialize, Serialize};

    let mut group = c.benchmark_group("message_serialization");

    let op = create_test_op(100, 42);

    // Benchmark serialization
    group.bench_function("serialize_attested_op", |b| {
        b.iter(|| {
            let bytes = bincode::serialize(black_box(&op)).unwrap();
            black_box(bytes);
        });
    });

    // Benchmark deserialization
    let serialized = bincode::serialize(&op).unwrap();
    group.bench_function("deserialize_attested_op", |b| {
        b.iter(|| {
            let op: AttestedOp = bincode::deserialize(black_box(&serialized)).unwrap();
            black_box(op);
        });
    });

    // Benchmark batch serialization
    for batch_size in [10, 50, 100].iter() {
        let ops: Vec<AttestedOp> = (0..*batch_size)
            .map(|i| create_test_op(i as u64, i as u32))
            .collect();

        group.bench_with_input(
            BenchmarkId::new("serialize_batch", batch_size),
            batch_size,
            |b, _| {
                b.iter(|| {
                    let bytes = bincode::serialize(black_box(&ops)).unwrap();
                    black_box(bytes);
                });
            },
        );
    }

    group.finish();
}

// ============================================================================
// Benchmark 6: Scalability Tests
// ============================================================================

fn bench_scalability(c: &mut Criterion) {
    let mut group = c.benchmark_group("scalability");

    // Test with many peers (simulates large network)
    for num_peers in [10, 50, 100].iter() {
        let view = create_peer_view_with_peers(*num_peers);

        group.bench_with_input(
            BenchmarkId::new("iterate_peers", num_peers),
            num_peers,
            |b, _| {
                b.iter(|| {
                    let count = view.peers().count();
                    black_box(count);
                });
            },
        );
    }

    // Test OpLog with many operations
    for num_ops in [100, 1000, 10000].iter() {
        let ops: Vec<AttestedOp> = (0..*num_ops)
            .map(|i| create_test_op(i as u64, i as u32))
            .collect();

        group.bench_with_input(BenchmarkId::new("iterate_ops", num_ops), num_ops, |b, _| {
            b.iter(|| {
                let count = black_box(&ops).len();
                black_box(count);
            });
        });
    }

    group.finish();
}

// ============================================================================
// Benchmark Registration
// ============================================================================

criterion_group!(
    benches,
    bench_peer_view_operations,
    bench_intent_state_operations,
    bench_digest_computation,
    bench_difference_calculation,
    bench_message_serialization,
    bench_scalability,
);

criterion_main!(benches);
