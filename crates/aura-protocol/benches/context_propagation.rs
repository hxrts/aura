//! Performance benchmarks for device and context operations
//!
//! These benchmarks measure:
//! - DeviceId creation and manipulation
//! - ContextId creation and manipulation
//! - AuthorityId operations

use aura_core::identifiers::{AuthorityId, DeviceId};
use aura_core::ContextId;
use criterion::{black_box, criterion_group, criterion_main, Criterion};

/// Benchmark DeviceId operations
fn bench_device_id_operations(c: &mut Criterion) {
    c.bench_function("device_id_from_bytes", |b| {
        b.iter(|| {
            let id = DeviceId::from_bytes([1u8; 32]);
            black_box(id);
        });
    });

    c.bench_function("device_id_to_bytes", |b| {
        let id = DeviceId::from_bytes([1u8; 32]);
        b.iter(|| {
            let bytes = id.to_bytes();
            black_box(bytes);
        });
    });

    c.bench_function("device_id_clone", |b| {
        let id = DeviceId::from_bytes([1u8; 32]);
        b.iter(|| {
            let cloned = id.clone();
            black_box(cloned);
        });
    });
}

/// Benchmark ContextId operations
fn bench_context_id_operations(c: &mut Criterion) {
    c.bench_function("context_id_new", |b| {
        b.iter(|| {
            let id = ContextId::new();
            black_box(id);
        });
    });

    c.bench_function("context_id_to_bytes", |b| {
        let id = ContextId::new();
        b.iter(|| {
            let bytes = id.to_bytes();
            black_box(bytes);
        });
    });

    c.bench_function("context_id_as_bytes", |b| {
        let id = ContextId::new();
        b.iter(|| {
            let bytes = id.as_bytes();
            black_box(bytes);
        });
    });

    c.bench_function("context_id_clone", |b| {
        let id = ContextId::new();
        b.iter(|| {
            let cloned = id.clone();
            black_box(cloned);
        });
    });
}

/// Benchmark AuthorityId operations
fn bench_authority_id_operations(c: &mut Criterion) {
    use uuid::Uuid;

    c.bench_function("authority_id_from_uuid", |b| {
        b.iter(|| {
            let uuid = Uuid::from_bytes([2u8; 16]);
            let id = AuthorityId::from_uuid(uuid);
            black_box(id);
        });
    });

    c.bench_function("authority_id_to_bytes", |b| {
        let uuid = Uuid::from_bytes([2u8; 16]);
        let id = AuthorityId::from_uuid(uuid);
        b.iter(|| {
            let bytes = id.to_bytes();
            black_box(bytes);
        });
    });

    c.bench_function("authority_id_clone", |b| {
        let uuid = Uuid::from_bytes([2u8; 16]);
        let id = AuthorityId::from_uuid(uuid);
        b.iter(|| {
            let cloned = id.clone();
            black_box(cloned);
        });
    });
}

/// Benchmark identifier equality checks
fn bench_identifier_equality(c: &mut Criterion) {
    let device1 = DeviceId::from_bytes([1u8; 32]);
    let device2 = DeviceId::from_bytes([1u8; 32]);
    let device3 = DeviceId::from_bytes([2u8; 32]);

    c.bench_function("device_id_equality_same", |b| {
        b.iter(|| {
            let result = device1 == device2;
            black_box(result);
        });
    });

    c.bench_function("device_id_equality_different", |b| {
        b.iter(|| {
            let result = device1 == device3;
            black_box(result);
        });
    });

    let context1 = ContextId::new();
    let context2 = context1.clone();

    c.bench_function("context_id_equality", |b| {
        b.iter(|| {
            let result = context1 == context2;
            black_box(result);
        });
    });
}

/// Benchmark identifier hashing
fn bench_identifier_hashing(c: &mut Criterion) {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    c.bench_function("device_id_hash", |b| {
        let id = DeviceId::from_bytes([1u8; 32]);
        b.iter(|| {
            let mut hasher = DefaultHasher::new();
            id.hash(&mut hasher);
            let hash = hasher.finish();
            black_box(hash);
        });
    });

    c.bench_function("context_id_hash", |b| {
        let id = ContextId::new();
        b.iter(|| {
            let mut hasher = DefaultHasher::new();
            id.hash(&mut hasher);
            let hash = hasher.finish();
            black_box(hash);
        });
    });

    c.bench_function("authority_id_hash", |b| {
        use uuid::Uuid;
        let uuid = Uuid::from_bytes([2u8; 16]);
        let id = AuthorityId::from_uuid(uuid);
        b.iter(|| {
            let mut hasher = DefaultHasher::new();
            id.hash(&mut hasher);
            let hash = hasher.finish();
            black_box(hash);
        });
    });
}

criterion_group!(
    benches,
    bench_device_id_operations,
    bench_context_id_operations,
    bench_authority_id_operations,
    bench_identifier_equality,
    bench_identifier_hashing
);

criterion_main!(benches);
