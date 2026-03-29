use async_lock::RwLock;
use std::sync::Arc;

use aura_app::AppCore;
use aura_terminal::handlers::tui::TuiMode;

use crate::support::{read_account_authority_id, read_account_config, IoContextTestEnvBuilder};

#[tokio::test]
async fn test_account_creation_callback_flow() {
    use aura_core::effects::StorageCoreEffects;
    use aura_effects::{
        EncryptedStorage, EncryptedStorageConfig, FilesystemStorageHandler, RealCryptoHandler,
        RealSecureStorageHandler,
    };

    let test_dir = std::env::temp_dir().join(format!("aura-callback-test-{}", std::process::id()));
    let env = IoContextTestEnvBuilder::new("callback")
        .with_base_path(test_dir.clone())
        .with_device_id("test-device-callback")
        .with_mode(TuiMode::Production)
        .build()
        .await;
    let account_file = test_dir.join("account.json.dat");

    assert!(!env.ctx.has_account(), "Should not have account initially");
    assert!(
        !account_file.exists(),
        "account.json.dat should not exist before creation"
    );

    let create_result = env.ctx.create_account("Bob").await;
    assert!(
        create_result.is_ok(),
        "create_account should succeed: {:?}",
        create_result
    );
    assert!(env.ctx.has_account(), "Should have account after creation");
    assert!(
        account_file.exists(),
        "account.json.dat MUST exist after create_account"
    );

    let storage = EncryptedStorage::new(
        FilesystemStorageHandler::from_path(test_dir.clone()),
        Arc::new(RealCryptoHandler::new()),
        Arc::new(RealSecureStorageHandler::with_base_path(test_dir.clone())),
        EncryptedStorageConfig::default(),
    );
    let content = storage
        .retrieve("account.json")
        .await
        .expect("Should be able to read account config from storage")
        .expect("account.json should exist in storage");
    assert!(content
        .windows(b"authority_id".len())
        .any(|window| window == b"authority_id"));
    assert!(content
        .windows(b"context_id".len())
        .any(|window| window == b"context_id"));

    let app_core2 = AppCore::new(aura_app::AppConfig::default()).expect("Failed to create AppCore");
    let app_core2 = Arc::new(RwLock::new(app_core2));
    let _initialized_app_core2 = aura_terminal::tui::context::InitializedAppCore::new(app_core2)
        .await
        .expect("Failed to init signals");

    let loaded_content: serde_json::Value =
        serde_json::from_slice(&content).expect("Should be valid JSON");
    assert!(loaded_content.get("authority_id").is_some());
    assert!(loaded_content.get("context_id").is_some());
}

#[tokio::test]
async fn test_device_id_determinism() {
    let device_id = "demo:bob";
    let test_dir =
        std::env::temp_dir().join(format!("aura-determinism-test-{}", std::process::id()));
    let account_file = test_dir.join("account.json.dat");

    let env = IoContextTestEnvBuilder::new("determinism-original")
        .with_base_path(test_dir.clone())
        .with_device_id(device_id)
        .with_mode(TuiMode::Production)
        .create_account_as("Bob")
        .build()
        .await;
    let original_authority_id = read_account_authority_id(&env.test_dir)
        .await
        .expect("Should read authority_id");

    std::fs::remove_file(&account_file).expect("Failed to delete account.json.dat");

    let recreated = IoContextTestEnvBuilder::new("determinism-recreated")
        .with_base_path(test_dir.clone())
        .with_device_id(device_id)
        .with_mode(TuiMode::Production)
        .create_account_as("Bob Again")
        .build()
        .await;
    let recreated_authority_id = read_account_authority_id(&recreated.test_dir)
        .await
        .expect("Should read recreated authority_id");
    assert_ne!(original_authority_id, recreated_authority_id);

    std::fs::remove_file(&account_file).expect("Failed to delete account.json.dat");

    let different = IoContextTestEnvBuilder::new("determinism-different")
        .with_base_path(test_dir.clone())
        .with_device_id("demo:bob-new-device")
        .with_mode(TuiMode::Production)
        .create_account_as("Bob New Device")
        .build()
        .await;
    let different_authority_id = read_account_authority_id(&different.test_dir)
        .await
        .expect("Should read different authority_id");
    assert_ne!(original_authority_id, different_authority_id);
}

#[tokio::test]
async fn test_guardian_recovery_preserves_cryptographic_identity() {
    let test_dir = std::env::temp_dir().join(format!(
        "aura-guardian-recovery-test-{}",
        std::process::id()
    ));
    let account_file = test_dir.join("account.json.dat");

    let original_env = IoContextTestEnvBuilder::new("guardian-original")
        .with_base_path(test_dir.clone())
        .with_device_id("bobs-original-phone-12345")
        .with_mode(TuiMode::Production)
        .create_account_as("Bob")
        .build()
        .await;
    let original_authority_id = read_account_authority_id(&original_env.test_dir)
        .await
        .expect("Should read original authority_id");

    std::fs::remove_file(&account_file).expect("Failed to delete account.json.dat");

    let wrong_env = IoContextTestEnvBuilder::new("guardian-wrong")
        .with_base_path(test_dir.clone())
        .with_device_id("bobs-replacement-phone-99999")
        .with_mode(TuiMode::Production)
        .create_account_as("Bob (New Device)")
        .build()
        .await;
    let wrong_authority_id = read_account_authority_id(&wrong_env.test_dir)
        .await
        .expect("Should read wrong authority_id");
    assert_ne!(original_authority_id, wrong_authority_id);

    std::fs::remove_file(&account_file).expect("Failed to delete wrong account.json.dat");

    let original_authority = original_authority_id
        .parse::<aura_core::types::identifiers::AuthorityId>()
        .expect("Invalid AuthorityId string");

    let recovered_env = IoContextTestEnvBuilder::new("guardian-recovered")
        .with_base_path(test_dir.clone())
        .with_device_id("bobs-replacement-phone-99999")
        .with_mode(TuiMode::Production)
        .build()
        .await;

    recovered_env
        .ctx
        .restore_recovered_account(original_authority, None)
        .await
        .expect("Failed to restore recovered account");

    let recovered_authority_id = read_account_authority_id(&recovered_env.test_dir)
        .await
        .expect("Should read recovered authority_id");
    assert_eq!(original_authority_id, recovered_authority_id);
    assert_ne!(recovered_authority_id, wrong_authority_id);

    let recovered_config = read_account_config(&recovered_env.test_dir)
        .await
        .expect("Should read recovered account config");
    assert_eq!(
        recovered_config["authority_id"].as_str(),
        Some(original_authority_id.as_str())
    );
}
