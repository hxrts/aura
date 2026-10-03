//! Browser profile lock and immutable secure-publication integration tests.
#![cfg(target_arch = "wasm32")]
use aura_core::effects::profile_storage::ProfileStorageError;
use aura_core::effects::secure::ImmutableSecureStoreOutcome;
use aura_core::effects::{SecureStorageCapability, SecureStorageEffects, SecureStorageLocation};
use aura_effects::profile_storage::FilesystemProfileStorageHandler;
use aura_effects::ProductionSecureStorageHandler;
use wasm_bindgen_test::*;
wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
async fn actual_web_lock_busy_then_acknowledged_release() {
    let handler = FilesystemProfileStorageHandler::new("test/profile-owner-browser".into());
    let owner = handler
        .acquire_owned_browser()
        .await
        .expect("browser must support actual profile ownership");
    assert!(matches!(
        handler.acquire_owned_browser().await,
        Err(ProfileStorageError::Busy)
    ));
    owner
        .release()
        .await
        .expect("browser releases actual request with acknowledgement");
    let owner = handler
        .acquire_owned_browser()
        .await
        .expect("exact profile reacquired after owner release");
    owner.release().await.unwrap();
}

#[wasm_bindgen_test]
async fn actual_idb_transactions_and_concurrent_first_wrapping_key_creation() {
    let profile = format!("test/immutable-browser-{}", js_sys::Math::random());
    let handler = FilesystemProfileStorageHandler::new(profile.clone().into());
    let owner = std::sync::Arc::new(handler.acquire_owned_browser().await.unwrap());
    let secure = ProductionSecureStorageHandler::for_production(profile.into())
        .retain_profile_owner(owner)
        .unwrap();
    let location = SecureStorageLocation::new("immutable_browser", "contended");
    let caps = [
        SecureStorageCapability::Write,
        SecureStorageCapability::Read,
    ];
    let (first, second) = futures::join!(
        secure.secure_store_immutable(&location, b"first", &caps),
        secure.secure_store_immutable(&location, b"second", &caps),
    );
    let outcomes = [first.unwrap(), second.unwrap()];
    assert_eq!(
        outcomes
            .iter()
            .filter(|o| **o == ImmutableSecureStoreOutcome::Created)
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|o| **o == ImmutableSecureStoreOutcome::AlreadyExists)
            .count(),
        1
    );
    let read = secure.secure_retrieve(&location, &caps).await.unwrap();
    assert!(
        read == b"first" || read == b"second",
        "winning encrypted record uses actual retained wrapping key"
    );
}

#[wasm_bindgen_test]
async fn historical_plaintext_namespace_cannot_initialize_a_fresh_crypto_profile() {
    let profile = format!("test/legacy-browser-{}", js_sys::Math::random());
    let secure_path = std::path::PathBuf::from(&profile).join("secure_store");
    let digest = aura_core::hash::hash(secure_path.to_string_lossy().as_bytes());
    let key = format!(
        "aura_storage_{}::signing_keys/actual_previous_authority:0/1",
        hex::encode(&digest[..8])
    );
    let storage = web_sys::window().unwrap().local_storage().unwrap().unwrap();
    storage
        .set_item(&key, "legacy encrypted-or-plaintext ambiguity")
        .unwrap();
    let adapter = FilesystemProfileStorageHandler::new(profile.clone().into());
    let owner = std::sync::Arc::new(adapter.acquire_owned_browser().await.unwrap());
    let rejected =
        ProductionSecureStorageHandler::for_production(profile.into()).retain_profile_owner(owner);
    assert!(matches!(
        rejected,
        Err(ProfileStorageError::LegacyBrowserSecureStorage { .. })
    ));
    assert!(
        storage.get_item(&key).unwrap().is_some(),
        "legacy material is not silently deleted or migrated"
    );
    storage.remove_item(&key).unwrap();
}
