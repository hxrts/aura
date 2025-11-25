#![allow(missing_docs)]
//! Performance benchmarks for individual effect handlers
//!
//! These benchmarks measure:
//! - Handler invocation overhead
//! - Direct handler operations
//! - Handler creation and composition

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::sync::Arc;
use tokio::runtime::Runtime;

use aura_core::effects::{CryptoEffects, RandomEffects, StorageEffects};
use aura_effects::{RealCryptoHandler, RealRandomHandler};
use aura_testkit::stateful_effects::{
    crypto::MockCryptoHandler, random::MockRandomHandler, storage::MemoryStorageHandler,
};

/// Benchmark random handler performance
fn bench_random_handlers(c: &mut Criterion) {
    let runtime = Runtime::new().unwrap_or_else(|_| panic!("Failed to create runtime"));

    c.bench_function("mock_random_handler_uuid", |b| {
        let handler = MockRandomHandler::new_with_seed(42);
        b.to_async(&runtime).iter(|| async {
            let uuid = handler.random_uuid().await;
            black_box(uuid);
        });
    });

    c.bench_function("real_random_handler_uuid", |b| {
        let handler = RealRandomHandler::new();
        b.to_async(&runtime).iter(|| async {
            let uuid = handler.random_uuid().await;
            black_box(uuid);
        });
    });

    c.bench_function("mock_random_handler_bytes", |b| {
        let handler = MockRandomHandler::new_with_seed(42);
        b.to_async(&runtime).iter(|| async {
            let bytes = handler.random_bytes(32).await;
            black_box(bytes);
        });
    });

    c.bench_function("real_random_handler_bytes", |b| {
        let handler = RealRandomHandler::new();
        b.to_async(&runtime).iter(|| async {
            let bytes = handler.random_bytes(32).await;
            black_box(bytes);
        });
    });
}

/// Benchmark crypto handler performance
fn bench_crypto_handlers(c: &mut Criterion) {
    let runtime = Runtime::new().unwrap_or_else(|_| panic!("Failed to create runtime"));

    c.bench_function("mock_crypto_ed25519_keypair", |b| {
        let handler = MockCryptoHandler::new();
        b.to_async(&runtime).iter(|| async {
            let keypair = handler.ed25519_generate_keypair().await;
            let _ = black_box(keypair);
        });
    });

    c.bench_function("real_crypto_ed25519_keypair", |b| {
        let handler = RealCryptoHandler::new();
        b.to_async(&runtime).iter(|| async {
            let keypair = handler.ed25519_generate_keypair().await;
            let _ = black_box(keypair);
        });
    });
}

/// Benchmark storage handler performance
fn bench_storage_handlers(c: &mut Criterion) {
    let runtime = Runtime::new().unwrap_or_else(|_| panic!("Failed to create runtime"));

    c.bench_function("memory_storage_store", |b| {
        let handler = MemoryStorageHandler::new();
        b.to_async(&runtime).iter(|| async {
            let result = handler.store("test_key", vec![0u8; 1024]).await;
            let _ = black_box(result);
        });
    });

    c.bench_function("memory_storage_retrieve", |b| {
        let handler = MemoryStorageHandler::new();
        runtime.block_on(async {
            let _ = handler.store("test_key", vec![0u8; 1024]).await;
        });

        b.to_async(&runtime).iter(|| async {
            let result = handler.retrieve("test_key").await;
            let _ = black_box(result);
        });
    });
}

/// Benchmark trait object overhead
fn bench_trait_object_overhead(c: &mut Criterion) {
    let runtime = Runtime::new().unwrap_or_else(|_| panic!("Failed to create runtime"));

    c.bench_function("direct_handler_call", |b| {
        let handler = MockRandomHandler::new_with_seed(42);
        b.to_async(&runtime).iter(|| async {
            let uuid = handler.random_uuid().await;
            black_box(uuid);
        });
    });

    c.bench_function("arc_trait_object_call", |b| {
        let handler: Arc<dyn RandomEffects> = Arc::new(MockRandomHandler::new_with_seed(42));
        b.to_async(&runtime).iter(|| async {
            let uuid = handler.random_uuid().await;
            black_box(uuid);
        });
    });

    c.bench_function("arc_clone_overhead", |b| {
        let handler: Arc<dyn RandomEffects> = Arc::new(MockRandomHandler::new_with_seed(42));
        b.iter(|| {
            let cloned = handler.clone();
            black_box(cloned);
        });
    });
}

/// Benchmark handler creation
fn bench_handler_creation(c: &mut Criterion) {
    c.bench_function("mock_random_creation", |b| {
        b.iter(|| {
            let handler = MockRandomHandler::new_with_seed(42);
            black_box(handler);
        });
    });

    c.bench_function("real_random_creation", |b| {
        b.iter(|| {
            let handler = RealRandomHandler::new();
            black_box(handler);
        });
    });

    c.bench_function("mock_crypto_creation", |b| {
        b.iter(|| {
            let handler = MockCryptoHandler::new();
            black_box(handler);
        });
    });

    c.bench_function("memory_storage_creation", |b| {
        b.iter(|| {
            let handler = MemoryStorageHandler::new();
            black_box(handler);
        });
    });
}

// Register all handler performance benchmarks for criterion execution
criterion_group!(
    benches,
    bench_random_handlers,
    bench_crypto_handlers,
    bench_storage_handlers,
    bench_trait_object_overhead,
    bench_handler_creation
);

criterion_main!(benches);
