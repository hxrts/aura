#![allow(missing_docs)]

use aura_journal::commitment_tree::authority_state::AuthorityTreeState;
use aura_journal::LeafId;
use criterion::{black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};
use rand_chacha::rand_core::{RngCore, SeedableRng};
use rand_chacha::ChaCha20Rng;

fn key_bytes(seed: u64, index: u32) -> Vec<u8> {
    let mut rng = ChaCha20Rng::seed_from_u64(seed ^ u64::from(index));
    let mut key = vec![0u8; 32];
    rng.fill_bytes(&mut key);
    key
}

fn build_state(device_count: u32) -> AuthorityTreeState {
    let mut state = AuthorityTreeState::new();
    for i in 0..device_count {
        state.add_device(key_bytes(0xFACE_B00C, i));
    }
    state
}

fn bench_incremental_vs_full(c: &mut Criterion) {
    let mut group = c.benchmark_group("authority_tree_incremental_vs_full");

    for n in [32_u32, 128, 512, 2048] {
        let base_state = build_state(n);
        let leaf = LeafId(n / 2);
        let new_key = key_bytes(0x1234_5678, n);

        group.bench_with_input(
            BenchmarkId::new("incremental_leaf_update", n),
            &base_state,
            |b, state| {
                b.iter_batched(
                    || state.clone(),
                    |mut candidate| {
                        candidate
                            .update_leaf_public_key(leaf, new_key.clone())
                            .expect("leaf update should succeed");
                        black_box(candidate.root_commitment);
                    },
                    BatchSize::SmallInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("full_recompute_reference", n),
            &base_state,
            |b, state| {
                b.iter_batched(
                    || state.clone(),
                    |mut candidate| {
                        candidate
                            .update_leaf_public_key(leaf, new_key.clone())
                            .expect("leaf update should succeed");
                        let root = candidate.recompute_root_commitment_full();
                        black_box(root);
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_incremental_vs_full);
criterion_main!(benches);
