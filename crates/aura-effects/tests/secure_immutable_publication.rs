//! Complete encrypted immutable records under actual process contention.
#![cfg(not(target_arch = "wasm32"))]
use aura_core::effects::secure::ImmutableSecureStoreOutcome;
use aura_core::effects::{SecureStorageCapability, SecureStorageEffects, SecureStorageLocation};
use aura_effects::FilesystemFallbackSecureStorageHandler;

#[tokio::test]
async fn immutable_record_never_replaces_existing_ciphertext() {
    let profile = tempfile::tempdir().unwrap();
    let handler = FilesystemFallbackSecureStorageHandler::with_base_path(profile.path().into());
    let location = SecureStorageLocation::new("admission_test", "actual_record");
    let caps = [
        SecureStorageCapability::Write,
        SecureStorageCapability::Read,
    ];
    assert_eq!(
        handler
            .secure_store_immutable(&location, b"original", &caps)
            .await
            .unwrap(),
        ImmutableSecureStoreOutcome::Created
    );
    assert_eq!(
        handler
            .secure_store_immutable(&location, b"replacement", &caps)
            .await
            .unwrap(),
        ImmutableSecureStoreOutcome::AlreadyExists
    );
    assert_eq!(
        handler.secure_retrieve(&location, &caps).await.unwrap(),
        b"original"
    );
    drop(handler);
    let restarted = FilesystemFallbackSecureStorageHandler::with_base_path(profile.path().into());
    assert_eq!(
        restarted.secure_retrieve(&location, &caps).await.unwrap(),
        b"original"
    );
}

#[test]
fn child_immutable_writer() {
    let Some(path) = std::env::var_os("AURA_IMMUTABLE_CHILD_PROFILE") else {
        return;
    };
    let value = std::env::var("AURA_IMMUTABLE_CHILD_VALUE").unwrap();
    let handler = FilesystemFallbackSecureStorageHandler::with_base_path(path.into());
    let location = SecureStorageLocation::new("admission_test", "contended");
    let result = futures::executor::block_on(handler.secure_store_immutable(
        &location,
        value.as_bytes(),
        &[SecureStorageCapability::Write],
    ))
    .unwrap();
    println!("IMMUTABLE_RESULT={result:?}");
}

#[tokio::test]
async fn actual_processes_publish_exactly_one_complete_record() {
    let profile = tempfile::tempdir().unwrap();
    let handler = FilesystemFallbackSecureStorageHandler::with_base_path(profile.path().into());
    let spawn = |value: &str| {
        tokio::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "child_immutable_writer", "--nocapture"])
            .env("AURA_IMMUTABLE_CHILD_PROFILE", profile.path())
            .env("AURA_IMMUTABLE_CHILD_VALUE", value)
            .stdout(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap()
    };
    let first = spawn("first");
    let second = spawn("second");
    let (first, second) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        tokio::join!(first.wait_with_output(), second.wait_with_output())
    })
    .await
    .expect("bounded actual process publication");
    let first = first.unwrap();
    let second = second.unwrap();
    assert!(first.status.success() && second.status.success());
    let results =
        String::from_utf8(first.stdout).unwrap() + &String::from_utf8(second.stdout).unwrap();
    assert_eq!(results.matches("IMMUTABLE_RESULT=Created").count(), 1);
    assert_eq!(results.matches("IMMUTABLE_RESULT=AlreadyExists").count(), 1);
    let stored = futures::executor::block_on(handler.secure_retrieve(
        &SecureStorageLocation::new("admission_test", "contended"),
        &[SecureStorageCapability::Read],
    ))
    .unwrap();
    assert!(stored == b"first" || stored == b"second");
}
