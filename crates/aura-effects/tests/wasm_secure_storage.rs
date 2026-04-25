#![cfg(target_arch = "wasm32")]
//! Browser regression tests for the WASM secure-storage backend.

use aura_core::effects::{SecureStorageCapability, SecureStorageEffects, SecureStorageLocation};
use aura_effects::FilesystemFallbackSecureStorageHandler;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use indexed_db_futures::{
    database::Database, prelude::*, query_source::QuerySource, transaction::TransactionMode,
};
use js_sys::Uint8Array;
use std::path::PathBuf;
use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

const RECORD_STORE: &str = "secure_records";

fn secure_db_name(base_path: &PathBuf) -> String {
    let path = base_path.to_string_lossy();
    let digest = aura_core::hash::hash(path.as_bytes());
    format!("hxrts_aura_secure_storage_{}", hex::encode(&digest[..8]))
}

fn assert_not_visible(
    label: &str,
    bytes: &[u8],
    secret: &[u8],
    secret_hex: &str,
    secret_base64: &str,
) {
    assert!(
        !bytes.windows(secret.len()).any(|window| window == secret),
        "{label} exposed raw secure-storage secret bytes"
    );
    let text = String::from_utf8_lossy(bytes);
    assert!(
        !text.contains(secret_hex),
        "{label} exposed hex-encoded secure-storage secret bytes"
    );
    assert!(
        !text.contains(secret_base64),
        "{label} exposed base64-encoded secure-storage secret bytes"
    );
}

#[wasm_bindgen_test(async)]
async fn secure_storage_does_not_expose_secret_in_browser_storage() {
    let base_path = PathBuf::from("aura-wasm-secure-storage-browser-regression");
    let db_name = secure_db_name(&base_path);
    if let Ok(delete_old) = Database::delete_by_name(&db_name) {
        let _ = delete_old.await;
    }

    let handler = FilesystemFallbackSecureStorageHandler::with_base_path(base_path.clone());
    let location = SecureStorageLocation::new("browser_regression", "known_secret");
    let caps = [
        SecureStorageCapability::Read,
        SecureStorageCapability::Write,
        SecureStorageCapability::Delete,
        SecureStorageCapability::List,
    ];
    let secret = b"aura-known-browser-secret-plain-bytes";
    let secret_hex = hex::encode(secret);
    let secret_base64 = BASE64_STANDARD.encode(secret);

    handler
        .secure_store(&location, secret, &caps)
        .await
        .expect("store secure browser secret");
    let retrieved = handler
        .secure_retrieve(&location, &caps)
        .await
        .expect("retrieve secure browser secret");
    assert_eq!(retrieved, secret);

    let storage = web_sys::window()
        .expect("window")
        .local_storage()
        .expect("localStorage lookup")
        .expect("localStorage");
    for index in 0..storage.length().expect("localStorage length") {
        if let Some(key) = storage.key(index).expect("localStorage key") {
            assert_not_visible(
                "localStorage key",
                key.as_bytes(),
                secret,
                &secret_hex,
                &secret_base64,
            );
            if let Some(value) = storage.get_item(&key).expect("localStorage value") {
                assert_not_visible(
                    "localStorage value",
                    value.as_bytes(),
                    secret,
                    &secret_hex,
                    &secret_base64,
                );
            }
        }
    }

    let db = Database::open(&db_name)
        .with_version(1u8)
        .await
        .expect("open secure storage db");
    let transaction = db.transaction(RECORD_STORE).build().expect("read tx");
    let store = transaction
        .object_store(RECORD_STORE)
        .expect("record store");

    let keys = store
        .get_all_keys::<String>()
        .primitive()
        .expect("read keys request")
        .await
        .expect("read keys");
    for key in keys {
        let key = key.expect("decode IndexedDB key");
        assert_not_visible(
            "IndexedDB record key",
            key.as_bytes(),
            secret,
            &secret_hex,
            &secret_base64,
        );
    }

    let records = store
        .get_all::<JsValue>()
        .primitive()
        .expect("read records request")
        .await
        .expect("read records");
    for record in records {
        let record = record.expect("decode IndexedDB record");
        let bytes = Uint8Array::new(&record).to_vec();
        assert_not_visible(
            "IndexedDB encrypted record",
            &bytes,
            secret,
            &secret_hex,
            &secret_base64,
        );
    }

    handler
        .secure_delete(&location, &caps)
        .await
        .expect("delete secure browser secret");
    let cleanup = db
        .transaction(RECORD_STORE)
        .with_mode(TransactionMode::Readwrite)
        .build()
        .expect("cleanup tx");
    cleanup
        .object_store(RECORD_STORE)
        .expect("cleanup store")
        .clear()
        .expect("clear request")
        .await
        .expect("clear records");
    cleanup.commit().await.expect("cleanup commit");
}
