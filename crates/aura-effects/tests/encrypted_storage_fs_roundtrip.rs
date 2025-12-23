#![allow(clippy::expect_used, clippy::unwrap_used)]

//! Filesystem-backed encryption-at-rest integration test.

use std::sync::Arc;

use aura_core::effects::StorageEffects;
use aura_effects::{
    EncryptedStorage, EncryptedStorageConfig, FilesystemStorageHandler, RealCryptoHandler,
    RealSecureStorageHandler,
};

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[tokio::test]
async fn encrypted_storage_fs_round_trip_is_not_plaintext_on_disk() {
    let dir = tempfile::tempdir().expect("tempdir");

    let storage_root = dir.path().join("storage");
    let secure_root = dir.path().to_path_buf();

    let storage = FilesystemStorageHandler::new(storage_root.clone());
    let crypto = Arc::new(RealCryptoHandler::new());
    let secure = Arc::new(RealSecureStorageHandler::with_base_path(secure_root));

    let encrypted =
        EncryptedStorage::new(storage, crypto, secure, EncryptedStorageConfig::default());

    let key = "fs-roundtrip";
    let marker = b"AURA_PLAINTEXT_MARKER__DO_NOT_LEAK__AURA_PLAINTEXT_MARKER";
    let value = [marker.as_slice(), b"::payload"].concat();

    encrypted
        .store(key, value.clone())
        .await
        .expect("store should succeed");

    let retrieved = encrypted
        .retrieve(key)
        .await
        .expect("retrieve should succeed")
        .expect("value should exist");

    assert_eq!(retrieved, value);

    // Ensure the on-disk blob is not the plaintext marker.
    let raw_path = storage_root.join(format!("{}.dat", key));
    let raw = std::fs::read(&raw_path).expect("read raw blob");

    assert!(
        !contains_subslice(&raw, marker),
        "on-disk ciphertext unexpectedly contains plaintext marker"
    );
}
