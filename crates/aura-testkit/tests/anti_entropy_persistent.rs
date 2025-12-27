use aura_anti_entropy::{PersistentSyncHandler, SyncEffects};
use aura_testkit::MemoryStorageHandler;
use std::sync::Arc;

#[tokio::test]
async fn test_persistent_sync_handler_empty_init() {
    let storage = Arc::new(MemoryStorageHandler::new());
    let handler = PersistentSyncHandler::new(storage);

    let digest = handler.get_oplog_digest().await.unwrap();
    assert!(digest.is_empty());

    let ops = handler.get_missing_ops(&digest).await.unwrap();
    assert!(ops.is_empty());
}

#[tokio::test]
async fn test_persistent_sync_handler_lazy_init() {
    let storage = Arc::new(MemoryStorageHandler::new());
    let handler = PersistentSyncHandler::new(storage);

    // Access digest triggers initialization
    let digest = handler.get_oplog_digest().await.unwrap();
    assert!(digest.is_empty());
}

#[tokio::test]
async fn test_persistent_sync_handler_survives_restart() {
    let storage = Arc::new(MemoryStorageHandler::new());

    // Create first handler
    let handler1 = PersistentSyncHandler::new(storage.clone());
    let digest = handler1.get_oplog_digest().await.unwrap();
    assert!(digest.is_empty());
    drop(handler1);

    // Create second handler with same storage
    let handler2 = PersistentSyncHandler::new(storage);
    let digest = handler2.get_oplog_digest().await.unwrap();
    assert!(digest.is_empty());

    let ops = handler2.get_missing_ops(&digest).await.unwrap();
    assert!(ops.is_empty());
}
