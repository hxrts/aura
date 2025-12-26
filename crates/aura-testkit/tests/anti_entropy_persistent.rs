use aura_anti_entropy::PersistentSyncHandler;
use aura_testkit::MemoryStorageHandler;
use std::sync::Arc;

#[tokio::test]
async fn test_persistent_sync_handler_empty_init() {
    let storage = Arc::new(MemoryStorageHandler::new());
    let handler = PersistentSyncHandler::new(storage);

    handler.ensure_initialized().await.unwrap();

    let ops = handler.ops_cache.read().await;
    assert!(ops.is_empty());
}

#[tokio::test]
async fn test_persistent_sync_handler_lazy_init() {
    let storage = Arc::new(MemoryStorageHandler::new());
    let handler = PersistentSyncHandler::new(storage);

    // Not initialized yet
    assert!(!handler
        .initialized
        .load(std::sync::atomic::Ordering::Acquire));

    // Access digest triggers initialization
    let digest = handler.get_oplog_digest().await.unwrap();
    assert!(digest.is_empty());

    // Now initialized
    assert!(handler
        .initialized
        .load(std::sync::atomic::Ordering::Acquire));
}

#[tokio::test]
async fn test_persistent_sync_handler_survives_restart() {
    let storage = Arc::new(MemoryStorageHandler::new());

    // Create first handler
    let handler1 = PersistentSyncHandler::new(storage.clone());
    handler1.ensure_initialized().await.unwrap();
    drop(handler1);

    // Create second handler with same storage
    let handler2 = PersistentSyncHandler::new(storage);
    handler2.ensure_initialized().await.unwrap();

    let ops = handler2.ops_cache.read().await;
    assert!(ops.is_empty());
}
