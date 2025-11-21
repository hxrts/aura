//! Transport Layer Performance Benchmarks
//!
//! These benchmarks measure:
//! - Envelope creation and manipulation
//! - Message serialization/deserialization
//! - Privacy level operations

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use aura_transport::{Envelope, ScopedEnvelope, PrivacyLevel};
use aura_core::{identifiers::DeviceId, RelationshipId};

/// Benchmark envelope creation
fn bench_envelope_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("envelope_creation");

    for size in [256, 1024, 4096, 16384] {
        let payload = vec![0u8; size];

        group.bench_with_input(BenchmarkId::new("new", size), &payload, |b, payload| {
            b.iter(|| {
                let envelope = Envelope::new(payload.clone());
                black_box(envelope);
            });
        });

        group.bench_with_input(BenchmarkId::new("new_blinded", size), &payload, |b, payload| {
            b.iter(|| {
                let envelope = Envelope::new_blinded(payload.clone());
                black_box(envelope);
            });
        });

        let relationship_id = RelationshipId::new([3u8; 32]);
        group.bench_with_input(BenchmarkId::new("new_scoped", size), &payload, |b, payload| {
            b.iter(|| {
                let envelope = Envelope::new_scoped(payload.clone(), relationship_id, None);
                black_box(envelope);
            });
        });
    }

    group.finish();
}

/// Benchmark envelope serialization
fn bench_envelope_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("envelope_serialization");

    for size in [256, 1024, 4096] {
        let payload = vec![0u8; size];
        let envelope = Envelope::new(payload);

        group.bench_with_input(BenchmarkId::new("serialize", size), &envelope, |b, env| {
            b.iter(|| {
                let serialized = serde_json::to_vec(env).unwrap();
                black_box(serialized);
            });
        });

        let serialized = serde_json::to_vec(&envelope).unwrap();
        group.bench_with_input(BenchmarkId::new("deserialize", size), &serialized, |b, data| {
            b.iter(|| {
                let env: Envelope = serde_json::from_slice(data).unwrap();
                black_box(env);
            });
        });
    }

    group.finish();
}

/// Benchmark scoped envelope operations
fn bench_scoped_envelope(c: &mut Criterion) {
    let payload = vec![0u8; 1024];
    let relationship_id = RelationshipId::new([1u8; 32]);
    let sender = DeviceId::from_bytes([2u8; 32]);
    let recipient = DeviceId::from_bytes([3u8; 32]);

    c.bench_function("scoped_envelope_creation", |b| {
        b.iter(|| {
            let envelope = Envelope::new(payload.clone());
            let scoped = ScopedEnvelope::new(envelope, relationship_id, sender, recipient);
            black_box(scoped);
        });
    });

    let envelope = Envelope::new(payload.clone());
    let scoped = ScopedEnvelope::new(envelope, relationship_id, sender, recipient);

    c.bench_function("scoped_envelope_verify_sender", |b| {
        b.iter(|| {
            let result = scoped.verify_sender(sender);
            black_box(result);
        });
    });

    c.bench_function("scoped_envelope_into_envelope", |b| {
        b.iter(|| {
            let cloned = scoped.clone();
            let envelope = cloned.into_envelope();
            black_box(envelope);
        });
    });
}

/// Benchmark privacy level checks
fn bench_privacy_operations(c: &mut Criterion) {
    let clear_envelope = Envelope::new(vec![0u8; 1024]);
    let blinded_envelope = Envelope::new_blinded(vec![0u8; 1024]);
    let scoped_envelope = Envelope::new_scoped(
        vec![0u8; 1024],
        RelationshipId::new([1u8; 32]),
        None
    );

    c.bench_function("privacy_level_clear", |b| {
        b.iter(|| {
            let level = clear_envelope.privacy_level();
            black_box(level);
        });
    });

    c.bench_function("privacy_level_blinded", |b| {
        b.iter(|| {
            let level = blinded_envelope.privacy_level();
            black_box(level);
        });
    });

    c.bench_function("privacy_level_scoped", |b| {
        b.iter(|| {
            let level = scoped_envelope.privacy_level();
            black_box(level);
        });
    });

    c.bench_function("requires_relationship_scope", |b| {
        b.iter(|| {
            let result = scoped_envelope.requires_relationship_scope();
            black_box(result);
        });
    });
}

/// Benchmark envelope cloning
fn bench_envelope_cloning(c: &mut Criterion) {
    let mut group = c.benchmark_group("envelope_cloning");

    for size in [256, 1024, 4096] {
        let payload = vec![0u8; size];
        let envelope = Envelope::new(payload);

        group.bench_with_input(BenchmarkId::new("clone", size), &envelope, |b, env| {
            b.iter(|| {
                let cloned = env.clone();
                black_box(cloned);
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_envelope_creation,
    bench_envelope_serialization,
    bench_scoped_envelope,
    bench_privacy_operations,
    bench_envelope_cloning
);

criterion_main!(benches);
