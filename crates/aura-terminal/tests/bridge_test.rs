//! # Effect Bridge Tests
//!
//! Unit tests for the EffectBridge implementation, covering:
//! - Bridge configuration
//! - Event filtering and subscription
//! - Connection state management
//! - Command authorization levels

use aura_terminal::tui::effects::{
    check_authorization, AuraEvent, BridgeConfig, CommandAuthorizationLevel, EffectBridge,
    EffectCommand, EventFilter,
};
use tokio::sync::broadcast;

#[test]
fn test_bridge_config_default() {
    let config = BridgeConfig::default();
    assert_eq!(config.command_buffer_size, 256);
    assert_eq!(config.event_buffer_size, 1024);
    assert!(config.auto_retry);
    assert_eq!(config.max_retries, 3);
}

#[test]
fn test_event_filter_all() {
    let filter = EventFilter::all();
    assert!(filter.connection);
    assert!(filter.recovery);
    assert!(filter.account);
    assert!(filter.chat);
    assert!(filter.sync);
    assert!(filter.block);
    assert!(filter.invitation);
    assert!(filter.settings);
    assert!(filter.moderation);
    assert!(filter.authorization);
    assert!(filter.errors);
    assert!(filter.system);
}

#[test]
fn test_event_filter_essential() {
    let filter = EventFilter::essential();
    assert!(filter.connection);
    assert!(!filter.recovery);
    assert!(!filter.account);
    assert!(!filter.chat);
    assert!(!filter.sync);
    assert!(!filter.block);
    assert!(!filter.invitation);
    assert!(!filter.settings);
    assert!(!filter.authorization);
    assert!(filter.errors);
    assert!(filter.system);
}

#[test]
fn test_event_filter_matches() {
    let recovery_filter = EventFilter::recovery();

    assert!(recovery_filter.matches(&AuraEvent::RecoveryStarted {
        session_id: "test".to_string()
    }));

    assert!(recovery_filter.matches(&AuraEvent::Error {
        code: "ERR".to_string(),
        message: "test".to_string()
    }));

    assert!(!recovery_filter.matches(&AuraEvent::Connected));

    assert!(!recovery_filter.matches(&AuraEvent::MessageReceived {
        channel: "test".to_string(),
        from: "user".to_string(),
        content: "hi".to_string(),
        timestamp: 0,
    }));
}

#[tokio::test]
async fn test_bridge_creation() {
    let bridge = EffectBridge::new();
    assert!(!bridge.is_connected().await);
    assert_eq!(bridge.pending_commands().await, 0);
    assert!(bridge.last_error().await.is_none());
}

#[tokio::test]
async fn test_bridge_connection_state() {
    let bridge = EffectBridge::new();

    bridge.set_connected(true).await;
    assert!(bridge.is_connected().await);

    bridge.set_connected(false).await;
    assert!(!bridge.is_connected().await);
}

#[tokio::test]
async fn test_bridge_error_state() {
    let bridge = EffectBridge::new();

    bridge.set_error("Test error").await;
    assert_eq!(bridge.last_error().await, Some("Test error".to_string()));

    bridge.clear_error().await;
    assert!(bridge.last_error().await.is_none());
}

#[tokio::test]
async fn test_bridge_emit_event() {
    let bridge = EffectBridge::new();
    let mut sub = bridge.subscribe(EventFilter::all());

    bridge.emit(AuraEvent::Connected);

    // Use try_recv to check if event was received
    // Note: In real usage, recv() would be used in an async context
    let event = sub.try_recv();
    assert!(matches!(event, Some(AuraEvent::Connected)));
}

#[tokio::test]
async fn test_subscription_filter() {
    let bridge = EffectBridge::new();
    let mut recovery_sub = bridge.subscribe(EventFilter::recovery());

    // Emit non-matching event
    bridge.emit(AuraEvent::Connected);

    // Emit matching event
    bridge.emit(AuraEvent::RecoveryStarted {
        session_id: "test".to_string(),
    });

    // Should only receive the recovery event
    let event = recovery_sub.try_recv();
    assert!(matches!(event, Some(AuraEvent::RecoveryStarted { .. })));
}

// === Authorization Tests ===

#[test]
fn test_authorization_level_ordering() {
    // Verify the ordering: Public < Basic < Sensitive < Admin
    assert!(CommandAuthorizationLevel::Public < CommandAuthorizationLevel::Basic);
    assert!(CommandAuthorizationLevel::Basic < CommandAuthorizationLevel::Sensitive);
    assert!(CommandAuthorizationLevel::Sensitive < CommandAuthorizationLevel::Admin);
}

#[test]
fn test_authorization_level_description() {
    assert_eq!(
        CommandAuthorizationLevel::Public.description(),
        "public access"
    );
    assert_eq!(
        CommandAuthorizationLevel::Admin.description(),
        "administrator privileges"
    );
}

#[test]
fn test_command_authorization_levels() {
    // Public commands
    assert_eq!(
        EffectCommand::RefreshAccount.authorization_level(),
        CommandAuthorizationLevel::Public
    );
    assert_eq!(
        EffectCommand::Ping.authorization_level(),
        CommandAuthorizationLevel::Public
    );

    // Basic commands
    assert_eq!(
        EffectCommand::SendMessage {
            channel: "test".to_string(),
            content: "hello".to_string()
        }
        .authorization_level(),
        CommandAuthorizationLevel::Basic
    );

    // Sensitive commands
    assert_eq!(
        EffectCommand::StartRecovery.authorization_level(),
        CommandAuthorizationLevel::Sensitive
    );
    assert_eq!(
        EffectCommand::CreateAccount {
            display_name: "test".to_string()
        }
        .authorization_level(),
        CommandAuthorizationLevel::Sensitive
    );

    // Admin commands
    assert_eq!(
        EffectCommand::Shutdown.authorization_level(),
        CommandAuthorizationLevel::Admin
    );
    assert_eq!(
        EffectCommand::KickUser {
            channel: "test".to_string(),
            target: "user".to_string(),
            reason: None
        }
        .authorization_level(),
        CommandAuthorizationLevel::Admin
    );
}

#[test]
fn test_check_authorization_public_always_passes() {
    let (event_tx, _rx) = broadcast::channel(16);
    let cmd = EffectCommand::RefreshAccount;

    // Even with Public user level, public commands pass
    let result = check_authorization(&cmd, CommandAuthorizationLevel::Public, &event_tx);
    assert!(result.is_ok());
}

#[test]
fn test_check_authorization_sufficient_level() {
    let (event_tx, _rx) = broadcast::channel(16);

    // Basic user can execute basic commands
    let cmd = EffectCommand::SendMessage {
        channel: "test".to_string(),
        content: "hello".to_string(),
    };
    let result = check_authorization(&cmd, CommandAuthorizationLevel::Basic, &event_tx);
    assert!(result.is_ok());

    // Admin user can execute basic commands
    let result = check_authorization(&cmd, CommandAuthorizationLevel::Admin, &event_tx);
    assert!(result.is_ok());
}

#[test]
fn test_check_authorization_insufficient_level() {
    let (event_tx, mut rx) = broadcast::channel(16);

    // Basic user cannot execute admin commands
    let cmd = EffectCommand::Shutdown;
    let result = check_authorization(&cmd, CommandAuthorizationLevel::Basic, &event_tx);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Authorization denied"));

    // Check that AuthorizationDenied event was emitted
    let event = rx.try_recv().unwrap();
    assert!(matches!(event, AuraEvent::AuthorizationDenied { .. }));
}
