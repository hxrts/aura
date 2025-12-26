use aura_protocol::PersistentTreeHandler;
use aura_testkit::MemoryStorageHandler;
use std::sync::Arc;

#[tokio::test]
async fn test_persistent_handler_empty_init() {
    let storage = Arc::new(MemoryStorageHandler::new());
    let handler = PersistentTreeHandler::new(storage);

    // Trigger lazy initialization
    handler.ensure_initialized().await.unwrap();

    // Should have no ops
    let ops = handler.ops_cache.read().expect("lock poisoned in test");
    assert!(ops.is_empty());
}

#[tokio::test]
async fn test_persistent_handler_lazy_init() {
    let storage = Arc::new(MemoryStorageHandler::new());
    let handler = PersistentTreeHandler::new(storage);

    // Not initialized yet
    assert!(!handler
        .initialized
        .load(std::sync::atomic::Ordering::Acquire));

    // Access state triggers initialization
    let _state = handler.get_current_state().await;

    // Now initialized
    assert!(handler
        .initialized
        .load(std::sync::atomic::Ordering::Acquire));
}

#[tokio::test]
async fn test_persistent_handler_survives_restart() {
    let storage = Arc::new(MemoryStorageHandler::new());

    // Create a handler
    let handler1 = PersistentTreeHandler::new(storage.clone());

    // Trigger initialization
    handler1.ensure_initialized().await.unwrap();

    // Verify initial state
    let ops1 = handler1.ops_cache.read().expect("lock poisoned in test");
    assert!(ops1.is_empty());
    drop(ops1);
    drop(handler1);

    // Create a new handler with same storage - should load same state
    let handler2 = PersistentTreeHandler::new(storage);
    handler2.ensure_initialized().await.unwrap();
    let ops2 = handler2.ops_cache.read().expect("lock poisoned in test");
    assert!(ops2.is_empty()); // Still empty since we didn't add any ops
}
