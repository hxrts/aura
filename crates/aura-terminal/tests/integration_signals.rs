#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]
//! # Signal Integration Tests
//!
//! Tests for the unified signal-based reactive architecture.
//! Verifies that read/emit operations work correctly with AppCore.
//!
//! Note: Subscription-based tests are excluded due to a timing race condition
//! in the subscribe implementation that spawns an async task before returning.
//! The subscription may not be fully established before emit is called.

use async_lock::RwLock;
use std::sync::Arc;

use aura_app::signal_defs::{
    ConnectionStatus, SyncStatus, CHAT_SIGNAL, CONNECTION_STATUS_SIGNAL, RECOVERY_SIGNAL,
    SYNC_STATUS_SIGNAL,
};
use aura_app::views::{Message, MessageDeliveryStatus, RecoveryProcess, RecoveryProcessStatus, RecoveryState};
use aura_app::{AppConfig, AppCore};
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::identifiers::{AuthorityId, ChannelId};

/// Helper to create a test AppCore with signals initialized
async fn test_app_core() -> Arc<RwLock<AppCore>> {
    let mut core = AppCore::new(AppConfig::default()).expect("Failed to create test AppCore");
    core.init_signals().await.expect("Failed to init signals");
    Arc::new(RwLock::new(core))
}

async fn wait_for_chat_signal(
    app_core: &Arc<RwLock<AppCore>>,
    mut predicate: impl FnMut(&aura_app::views::ChatState) -> bool,
) -> aura_app::views::ChatState {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(500);
    loop {
        let chat = {
            let core = app_core.read().await;
            core.read(&*CHAT_SIGNAL).await.unwrap()
        };

        if predicate(&chat) {
            return chat;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("Timed out waiting for chat signal to satisfy predicate");
        }

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

async fn wait_for_recovery_signal(
    app_core: &Arc<RwLock<AppCore>>,
    mut predicate: impl FnMut(&aura_app::views::RecoveryState) -> bool,
) -> aura_app::views::RecoveryState {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(500);
    loop {
        let recovery = {
            let core = app_core.read().await;
            core.read(&*RECOVERY_SIGNAL).await.unwrap()
        };

        if predicate(&recovery) {
            return recovery;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("Timed out waiting for recovery signal to satisfy predicate");
        }

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn test_signal_read_write_roundtrip() {
    let app_core = test_app_core().await;
    let core = app_core.read().await;

    // Read initial connection status
    let initial = core.read(&*CONNECTION_STATUS_SIGNAL).await.unwrap();
    assert!(matches!(initial, ConnectionStatus::Offline));

    // Emit new status
    core.emit(&*CONNECTION_STATUS_SIGNAL, ConnectionStatus::Connecting)
        .await
        .unwrap();

    // Read updated status
    let updated = core.read(&*CONNECTION_STATUS_SIGNAL).await.unwrap();
    assert!(matches!(updated, ConnectionStatus::Connecting));
}

#[tokio::test]
async fn test_sync_status_signal() {
    let app_core = test_app_core().await;
    let core = app_core.read().await;

    // Read initial sync status
    let initial = core.read(&*SYNC_STATUS_SIGNAL).await.unwrap();
    assert!(matches!(initial, SyncStatus::Idle));

    // Emit syncing status
    core.emit(&*SYNC_STATUS_SIGNAL, SyncStatus::Syncing { progress: 50 })
        .await
        .unwrap();

    // Read updated status
    let updated = core.read(&*SYNC_STATUS_SIGNAL).await.unwrap();
    assert!(matches!(updated, SyncStatus::Syncing { progress: 50 }));

    // Emit synced status
    core.emit(&*SYNC_STATUS_SIGNAL, SyncStatus::Synced)
        .await
        .unwrap();

    // Read final status
    let final_state = core.read(&*SYNC_STATUS_SIGNAL).await.unwrap();
    assert!(matches!(final_state, SyncStatus::Synced));
}

#[tokio::test]
async fn test_chat_signal_state_updates() {
    let app_core = test_app_core().await;
    let core = app_core.read().await;
    let test_channel_id = ChannelId::from_bytes([0x40; 32]);

    // Read initial chat state
    let initial = core.read(&*CHAT_SIGNAL).await.unwrap();
    assert!(initial.message_count() == 0);

    // Create updated state with a message
    let mut updated_state = initial.clone();
    updated_state.apply_message(test_channel_id, Message {
        id: "msg-1".to_string(),
        channel_id: test_channel_id,
        sender_id: AuthorityId::new_from_entropy([0xAA; 32]),
        sender_name: "Alice".to_string(),
        content: "Hello, world!".to_string(),
        timestamp: 1234567890,
        is_own: false,
        is_read: false,
        reply_to: None,
        delivery_status: MessageDeliveryStatus::default(),
        epoch_hint: None,
        is_finalized: false,
    });

    // Emit updated state
    core.emit(&*CHAT_SIGNAL, updated_state).await.unwrap();

    // Read and verify
    drop(core);
    let read_state = wait_for_chat_signal(&app_core, |chat| chat.message_count() == 1).await;
    let messages = read_state.messages_for_channel(&test_channel_id);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content, "Hello, world!");
}

#[tokio::test]
async fn test_recovery_signal_state_updates() {
    let app_core = test_app_core().await;
    let core = app_core.read().await;

    // Read initial recovery state
    let initial = core.read(&*RECOVERY_SIGNAL).await.unwrap();
    assert!(initial.active_recovery().is_none());

    // Create recovery state with active session using from_parts
    let updated_state = RecoveryState::from_parts(
        std::collections::HashMap::new(),
        2,
        Some(RecoveryProcess {
            id: "recovery-123".to_string(),
            account_id: AuthorityId::new_from_entropy([0x45; 32]),
            status: RecoveryProcessStatus::WaitingForApprovals,
            approvals_received: 0,
            approvals_required: 2,
            approved_by: vec![],
            approvals: vec![],
            initiated_at: 1234567890,
            expires_at: None,
            progress: 0,
        }),
        vec![],
        vec![],
    );

    // Emit updated state
    core.emit(&*RECOVERY_SIGNAL, updated_state).await.unwrap();

    // Read and verify
    drop(core);
    let read_state = wait_for_recovery_signal(&app_core, |r| r.active_recovery().is_some()).await;
    assert!(read_state.active_recovery().is_some());
    let active = read_state.active_recovery().unwrap();
    assert_eq!(active.id, "recovery-123");
    assert_eq!(active.approvals_required, 2);
}

#[tokio::test]
async fn test_signal_concurrent_access() {
    let app_core = test_app_core().await;

    // Spawn multiple tasks that read/write signals concurrently
    let handles: Vec<_> = (0..10)
        .map(|i| {
            let app_core = app_core.clone();
            tokio::spawn(async move {
                let core = app_core.read().await;

                // Each task emits a different progress value
                core.emit(
                    &*SYNC_STATUS_SIGNAL,
                    SyncStatus::Syncing { progress: i * 10 },
                )
                .await
                .unwrap();

                // And reads the current state
                let _ = core.read(&*SYNC_STATUS_SIGNAL).await.unwrap();
            })
        })
        .collect();

    // Wait for all tasks to complete
    for handle in handles {
        handle.await.unwrap();
    }

    // Final read should succeed (value will be one of the emitted values)
    let core = app_core.read().await;
    let final_state = core.read(&*SYNC_STATUS_SIGNAL).await.unwrap();
    // Just verify we can read without panic
    if let SyncStatus::Syncing { progress } = final_state {
        assert!(progress <= 90);
    }
}

#[tokio::test]
async fn test_connection_status_transitions() {
    let app_core = test_app_core().await;
    let core = app_core.read().await;

    // Test all connection status transitions
    let statuses = [
        ConnectionStatus::Offline,
        ConnectionStatus::Connecting,
        ConnectionStatus::Online { peer_count: 1 },
        ConnectionStatus::Online { peer_count: 5 },
        ConnectionStatus::Offline,
    ];

    for status in statuses {
        core.emit(&*CONNECTION_STATUS_SIGNAL, status.clone())
            .await
            .unwrap();
        let read = core.read(&*CONNECTION_STATUS_SIGNAL).await.unwrap();
        assert_eq!(read, status);
    }
}

#[tokio::test]
async fn test_chat_message_accumulation() {
    let app_core = test_app_core().await;
    let core = app_core.read().await;
    let test_channel_id = ChannelId::from_bytes([0x10; 32]);

    // Add multiple messages to same channel
    for i in 0..5 {
        let mut state = core.read(&*CHAT_SIGNAL).await.unwrap();
        state.apply_message(test_channel_id, Message {
            id: format!("msg-{i}"),
            channel_id: test_channel_id,
            sender_id: AuthorityId::new_from_entropy([0x20 + (i % 3) as u8; 32]),
            sender_name: format!("User{}", i % 3),
            content: format!("Message number {i}"),
            timestamp: 1234567890 + i as u64,
            is_own: i % 2 == 0,
            is_read: true,
            reply_to: None,
            delivery_status: MessageDeliveryStatus::default(),
            epoch_hint: None,
            is_finalized: false,
        });
        core.emit(&*CHAT_SIGNAL, state).await.unwrap();
    }

    // Verify all messages were accumulated
    drop(core);
    let final_state = wait_for_chat_signal(&app_core, |chat| chat.message_count() == 5).await;
    let messages = final_state.messages_for_channel(&test_channel_id);
    assert_eq!(messages.len(), 5);
    assert_eq!(messages[4].content, "Message number 4");
}
