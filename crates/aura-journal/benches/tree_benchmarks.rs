//! Performance benchmarks for ratchet tree operations.
//!
//! Measures performance characteristics of:
//! - Reduction time vs number of operations
//! - Commitment computation overhead
//! - Snapshot creation time vs tree size
//! - OpLog CRDT operations
//!
//! Run with: cargo bench --bench tree_benchmarks

use aura_core::tree::{
    AttestedOp, Epoch, Hash32, LeafId, LeafNode, LeafRole, NodeIndex, Policy, TreeOp, TreeOpKind,
};
use aura_journal::ratchet_tree::{reduction::reduce, state::TreeState};
use aura_journal::semilattice::OpLog;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use std::collections::BTreeMap;

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

fn create_oplog_with_ops(num_ops: usize) -> OpLog {
    let mut oplog = OpLog::new();
    for i in 0..num_ops {
        let op = create_test_op(i as u64, i as u32);
        oplog.add_operation(op);
    }
    oplog
}

// ============================================================================
// Benchmark 1: Reduction Time vs Number of Operations
// ============================================================================

fn bench_reduction_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("reduction_scaling");

    for size in [10, 50, 100, 500, 1000].iter() {
        let oplog = create_oplog_with_ops(*size);
        let ops: Vec<&AttestedOp> = oplog.list_ops();

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let _ = reduce(black_box(&ops));
            });
        });
    }

    group.finish();
}

// ============================================================================
// Benchmark 2: Commitment Computation
// ============================================================================

fn bench_commitment_computation(c: &mut Criterion) {
    use aura_core::tree::commitment::{commit_branch, commit_leaf};

    let mut group = c.benchmark_group("commitment_computation");

    // Benchmark leaf commitment
    group.bench_function("commit_leaf", |b| {
        b.iter(|| {
            let commitment = commit_leaf(
                black_box(&LeafId(42)),
                black_box(&Epoch(100)),
                black_box(&[0u8; 32]),
            );
            black_box(commitment);
        });
    });

    // Benchmark branch commitment
    group.bench_function("commit_branch", |b| {
        let policy = Policy::Threshold { m: 2, n: 3 };
        b.iter(|| {
            let commitment = commit_branch(
                black_box(&NodeIndex(5)),
                black_box(&Epoch(100)),
                black_box(&policy),
                black_box(&[0u8; 32]),
                black_box(&[1u8; 32]),
            );
            black_box(commitment);
        });
    });

    group.finish();
}

// ============================================================================
// Benchmark 3: OpLog CRDT Operations
// ============================================================================

fn bench_oplog_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("oplog_operations");

    // Benchmark OpLog add
    group.bench_function("oplog_add", |b| {
        let mut oplog = OpLog::new();
        let op = create_test_op(1, 1);

        b.iter(|| {
            oplog.add_operation(black_box(op.clone()));
        });
    });

    // Benchmark OpLog join (union of two logs)
    for size in [10, 50, 100].iter() {
        let oplog1 = create_oplog_with_ops(*size);
        let oplog2 = create_oplog_with_ops(*size);

        group.bench_with_input(BenchmarkId::new("oplog_join", size), size, |b, _| {
            b.iter(|| {
                let joined = black_box(&oplog1).join(black_box(&oplog2));
                black_box(joined);
            });
        });
    }

    // Benchmark OpLog contains check
    let oplog = create_oplog_with_ops(1000);
    let cid = compute_cid(&create_test_op(500, 500));

    group.bench_function("oplog_contains", |b| {
        b.iter(|| {
            let result = oplog.contains(black_box(&cid));
            black_box(result);
        });
    });

    group.finish();
}

// ============================================================================
// Benchmark 4: Tree State Operations
// ============================================================================

fn bench_tree_state_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("tree_state_operations");

    // Create a tree with some operations
    let oplog = create_oplog_with_ops(100);
    let ops: Vec<&AttestedOp> = oplog.list_ops();
    let state = reduce(&ops).unwrap();

    // Benchmark get_leaf
    group.bench_function("get_leaf", |b| {
        b.iter(|| {
            let leaf = state.get_leaf(black_box(&LeafId(50)));
            black_box(leaf);
        });
    });

    // Benchmark list_leaf_ids
    group.bench_function("list_leaf_ids", |b| {
        b.iter(|| {
            let leaf_ids = state.list_leaf_ids();
            black_box(leaf_ids);
        });
    });

    // Benchmark current_commitment
    group.bench_function("current_commitment", |b| {
        b.iter(|| {
            let commitment = state.current_commitment();
            black_box(commitment);
        });
    });

    group.finish();
}

// ============================================================================
// Benchmark 5: Snapshot Creation (Conceptual)
// ============================================================================

fn bench_snapshot_creation(c: &mut Criterion) {
    use aura_core::tree::snapshot::Snapshot;

    let mut group = c.benchmark_group("snapshot_creation");

    for tree_size in [10, 50, 100].iter() {
        let oplog = create_oplog_with_ops(*tree_size);
        let ops: Vec<&AttestedOp> = oplog.list_ops();
        let state = reduce(&ops).unwrap();

        group.bench_with_input(BenchmarkId::from_parameter(tree_size), tree_size, |b, _| {
            b.iter(|| {
                let snapshot = Snapshot {
                    epoch: state.epoch,
                    commitment: state.current_commitment(),
                    roster: state.list_leaf_ids(),
                    policies: BTreeMap::new(),
                    state_cid: None,
                    timestamp: 1000,
                    version: 1,
                };
                black_box(snapshot);
            });
        });
    }

    group.finish();
}

// ============================================================================
// Helper: Compute CID
// ============================================================================

fn compute_cid(op: &AttestedOp) -> Hash32 {
    use blake3::Hasher;
    let mut hasher = Hasher::new();
    hasher.update(b"ATTESTED_OP");
    hasher.update(&op.op.parent_epoch.0.to_le_bytes());
    hasher.update(&op.op.parent_commitment);
    hasher.update(&[op.op.version]);
    hasher.update(&(op.signer_count as u64).to_le_bytes());
    *hasher.finalize().as_bytes()
}

// ============================================================================
// Benchmark Registration
// ============================================================================

criterion_group!(
    benches,
    bench_reduction_scaling,
    bench_commitment_computation,
    bench_oplog_operations,
    bench_tree_state_operations,
    bench_snapshot_creation,
);

criterion_main!(benches);
