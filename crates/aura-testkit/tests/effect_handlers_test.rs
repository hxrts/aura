#![allow(missing_docs)]

//! Effect handler API smoke tests.
//!
//! This suite validates current handler contracts after crate refactors:
//! - Composite test handler wiring in testing mode
//! - In-memory storage semantics
//! - Real crypto sign/verify round-trip
//! - Console handler logging entry points

use aura_core::effects::{
    ConsoleEffects, CryptoCoreEffects, ExecutionMode, NetworkExtendedEffects, PhysicalTimeEffects,
    RandomCoreEffects, StorageCoreEffects, StorageExtendedEffects,
};
use aura_core::identifiers::DeviceId;
use aura_effects::{console::RealConsoleHandler, crypto::RealCryptoHandler};
use aura_testkit::{stateful_effects::storage::MemoryStorageHandler, CompositeTestHandler};

#[tokio::test]
async fn composite_handler_for_testing_exposes_core_effects() {
    let device_id = DeviceId::new_from_entropy([1u8; 32]);
    let handler = CompositeTestHandler::new_mock(ExecutionMode::Testing, device_id)
        .expect("build composite test handler");

    let peers = <CompositeTestHandler as NetworkExtendedEffects>::connected_peers(&handler).await;
    assert!(peers.is_empty());

    let now = PhysicalTimeEffects::physical_time(&handler)
        .await
        .expect("testing handler should provide physical time");
    assert!(now.ts_ms > 0);

    let rnd = RandomCoreEffects::random_bytes(&handler, 16).await;
    assert_eq!(rnd.len(), 16);
}

#[tokio::test]
async fn memory_storage_handler_round_trips_values_and_supports_batch() {
    let storage = MemoryStorageHandler::new();

    storage
        .store("k1", b"value-1".to_vec())
        .await
        .expect("store k1");
    assert_eq!(
        storage.retrieve("k1").await.expect("retrieve k1"),
        Some(b"value-1".to_vec())
    );
    assert!(storage.exists("k1").await.expect("exists k1"));

    let mut batch = std::collections::HashMap::new();
    batch.insert("k2".to_string(), b"value-2".to_vec());
    batch.insert("k3".to_string(), b"value-3".to_vec());
    storage.store_batch(batch).await.expect("store batch");

    let keys = storage.list_keys(Some("k")).await.expect("list keys");
    assert!(keys.iter().any(|k| k == "k1"));
    assert!(keys.iter().any(|k| k == "k2"));
    assert!(keys.iter().any(|k| k == "k3"));

    let removed = storage.remove("k1").await.expect("remove k1");
    assert!(removed);
    assert!(!storage.exists("k1").await.expect("k1 removed"));
}

#[tokio::test]
async fn real_crypto_handler_ed25519_round_trip() {
    let crypto = RealCryptoHandler::new();

    let (signing_key, verify_key) = crypto
        .ed25519_generate_keypair()
        .await
        .expect("keypair generation");
    let message = b"effect-handler-crypto-smoke";

    let signature = crypto
        .ed25519_sign(message, &signing_key)
        .await
        .expect("sign message");
    let is_valid = crypto
        .ed25519_verify(message, &signature, &verify_key)
        .await
        .expect("verify signature");
    assert!(is_valid);

    let is_invalid = crypto
        .ed25519_verify(b"tampered", &signature, &verify_key)
        .await
        .expect("verify tampered message");
    assert!(!is_invalid);
}

#[tokio::test]
async fn real_console_handler_accepts_log_operations() {
    let console = RealConsoleHandler::new();

    console
        .log_info("effect handler info")
        .await
        .expect("info log should succeed");
    console
        .log_warn("effect handler warn")
        .await
        .expect("warn log should succeed");
    console
        .log_error("effect handler error")
        .await
        .expect("error log should succeed");
}
