//! Actual browser writer, encrypted lifetime inventory, and strict commit custody.
#![cfg(target_arch = "wasm32")]
use aura_core::effects::secret_lifetime::SecretLifetimeState;
use aura_effects::profile_storage::FilesystemProfileStorageHandler;
use aura_effects::ProductionSecureStorageHandler;
use indexed_db_futures::{database::Database, prelude::*, transaction::TransactionMode};
use js_sys::Uint8Array;
use std::{path::PathBuf, sync::Arc};
use wasm_bindgen_test::*;
wasm_bindgen_test_configure!(run_in_browser);

fn selected(
    profile: &str,
    owner: Arc<aura_effects::profile_storage::OwnedProfileLease>,
) -> ProductionSecureStorageHandler {
    ProductionSecureStorageHandler::for_production(PathBuf::from(profile))
        .retain_profile_owner(owner)
        .expect("same selected browser profile")
}

#[wasm_bindgen_test(async)]
async fn selected_browser_lifetime_reopens_retired_inventory_and_rejects_foreign_lease() {
    let profile = format!("test/browser-lifetime-{}", js_sys::Math::random());
    let adapter = FilesystemProfileStorageHandler::new(profile.clone().into());
    let owner = Arc::new(
        adapter
            .acquire_owned_browser()
            .await
            .expect("actual Web Lock"),
    );
    let foreign_profile = format!("{profile}-foreign");
    let foreign_adapter = FilesystemProfileStorageHandler::new(foreign_profile.clone().into());
    let foreign_owner = Arc::new(
        foreign_adapter
            .acquire_owned_browser()
            .await
            .expect("foreign Web Lock"),
    );
    assert!(
        ProductionSecureStorageHandler::for_production(PathBuf::from(&profile))
            .retain_profile_owner(foreign_owner.clone())
            .is_err()
    );

    let duplicate = selected(&profile, owner.clone());
    let (secure, mut root) = selected(&profile, owner.clone())
        .into_selected_profile_lifetime_channel()
        .expect("selected lifetime channel");
    assert!(secure.owns_selected_profile_lifetime_channel(&root));
    assert!(
        duplicate.into_selected_profile_lifetime_channel().is_err(),
        "one-shot selected owner claim"
    );
    assert!(root
        .recover_owned_inventory()
        .await
        .expect("empty inventory")
        .is_empty());
    let allocation = root
        .allocate(b"original-rotation", b"retained-browser-secret")
        .await
        .expect("durable birth");
    let reference = allocation.reference().clone();
    assert!(matches!(
        allocation.state().await.unwrap(),
        SecretLifetimeState::Live
    ));
    let negative = allocation
        .decide_negative(b"cancelled")
        .await
        .expect("durable first decision");
    negative.retire().await.expect("durable retirement");
    assert!(matches!(
        allocation.state().await.unwrap(),
        SecretLifetimeState::Retired { .. }
    ));
    assert!(allocation.read_live_secret().await.is_err());
    drop(negative);
    drop(allocation);
    drop(root);
    drop(secure);
    drop(owner);

    let reopened_owner = Arc::new(
        adapter
            .acquire_owned_browser()
            .await
            .expect("reopened Web Lock"),
    );
    let (reopened_secure, mut reopened) = selected(&profile, reopened_owner.clone())
        .into_selected_profile_lifetime_channel()
        .expect("reopened selected channel");
    assert!(reopened_secure.owns_selected_profile_lifetime_channel(&reopened));
    let mut inventory = reopened
        .recover_owned_inventory()
        .await
        .expect("authenticated inventory");
    assert_eq!(inventory.len(), 1);
    let recovered = inventory.pop().unwrap();
    assert_eq!(recovered.reference(), &reference);
    assert!(matches!(
        recovered.state().await.unwrap(),
        SecretLifetimeState::Retired { .. }
    ));
    assert!(recovered.read_live_secret().await.is_err());
    drop(foreign_owner);
}

#[wasm_bindgen_test(async)]
async fn external_inventory_conflict_fails_closed_without_reissuing_secret() {
    let profile = format!("test/browser-lifetime-conflict-{}", js_sys::Math::random());
    let owner = Arc::new(
        FilesystemProfileStorageHandler::new(profile.clone().into())
            .acquire_owned_browser()
            .await
            .expect("actual Web Lock"),
    );
    let (_secure, mut root) = selected(&profile, owner)
        .into_selected_profile_lifetime_channel()
        .expect("selected provider");
    root.recover_owned_inventory()
        .await
        .expect("initial inventory");
    let allocation = root
        .allocate(b"rotation", b"original-secret")
        .await
        .expect("durable birth");
    let secure_path = PathBuf::from(&profile).join("secure_store");
    let digest = aura_core::hash::hash(secure_path.to_string_lossy().as_bytes());
    let db_name = format!("hxrts_aura_secure_storage_{}", hex::encode(&digest[..8]));
    let db = Database::open(&db_name)
        .with_version(1u8)
        .await
        .expect("actual IndexedDB");
    let transaction = db
        .transaction("secure_records")
        .with_mode(TransactionMode::Readwrite)
        .build()
        .expect("external conflict transaction");
    transaction
        .object_store("secure_records")
        .expect("actual record store")
        .put(wasm_bindgen::JsValue::from(Uint8Array::from(
            b"foreign-conflicting-record".as_slice(),
        )))
        .with_key(".allocation-lifetimes-v1/selected-profile-inventory")
        .primitive()
        .expect("external write request")
        .await
        .expect("external write");
    transaction
        .commit()
        .await
        .expect("external conflict committed");
    assert!(
        allocation.state().await.is_err(),
        "damaged authenticated original inventory must fail closed"
    );
    assert!(
        allocation.read_live_secret().await.is_err(),
        "no reissued secret after conflict"
    );
    assert!(
        root.reconcile_unhanded_births().await.is_err(),
        "no fresh empty inventory fallback"
    );
}
