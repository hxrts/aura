#![allow(missing_docs)]
use aura_journal::commitment_tree::TreeState;
use aura_protocol::effects::TreeEffects;
use aura_protocol::prelude::PersistentTreeHandler;
use aura_testkit::MemoryStorageHandler;
use std::sync::Arc;

#[tokio::test]
async fn test_persistent_handler_empty_init() {
    let storage = Arc::new(MemoryStorageHandler::new());
    let handler = PersistentTreeHandler::new(storage);

    // Trigger lazy initialization via public API
    let state = handler.get_current_state().await.unwrap();
    assert_eq!(state, TreeState::new());
}

#[tokio::test]
async fn test_persistent_handler_lazy_init() {
    let storage = Arc::new(MemoryStorageHandler::new());
    let handler = PersistentTreeHandler::new(storage);

    // Access state triggers initialization
    let state = handler.get_current_state().await.unwrap();
    assert_eq!(state, TreeState::new());
}

#[tokio::test]
async fn test_persistent_handler_survives_restart() {
    let storage = Arc::new(MemoryStorageHandler::new());

    // Create a handler
    let handler1 = PersistentTreeHandler::new(storage.clone());

    // Trigger initialization via public API
    let state1 = handler1.get_current_state().await.unwrap();
    assert_eq!(state1, TreeState::new());
    drop(handler1);

    // Create a new handler with same storage - should load same state
    let handler2 = PersistentTreeHandler::new(storage);
    let state2 = handler2.get_current_state().await.unwrap();
    assert_eq!(state2, TreeState::new()); // Still empty since we didn't add any ops
}
