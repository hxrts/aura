//! # Effect Types Tests
//!
//! Unit tests for TUI effect types, covering:
//! - Event filtering
//! - Command authorization levels

use aura_terminal::tui::effects::{
    AuraEvent, CommandAuthorizationLevel, EffectCommand, EventFilter,
};

#[test]
fn test_event_filter_all() {
    let filter = EventFilter::all();
    assert!(filter.connection);
    assert!(filter.recovery);
    assert!(filter.account);
    assert!(filter.chat);
    assert!(filter.sync);
    assert!(filter.home);
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
    assert!(!filter.home);
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

    // CreateAccount is Basic (not Sensitive) because it's the bootstrapping command
    // that users need to run before they have an account
    assert_eq!(
        EffectCommand::CreateAccount {
            display_name: "test".to_string()
        }
        .authorization_level(),
        CommandAuthorizationLevel::Basic
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
