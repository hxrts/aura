//! Channel lifecycle and membership tests for AMP.
//!
//! Tests the channel types, membership facts, and domain fact serialization.

use aura_amp::channel::{ChannelMembershipFact, ChannelParticipantEvent};
use aura_amp::config::AmpRuntimeConfig;
use aura_core::effects::amp::{
    AmpChannelEffects, ChannelCloseParams, ChannelCreateParams, ChannelJoinParams,
    ChannelLeaveParams, ChannelSendParams,
};
use aura_core::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_core::time::{OrderTime, TimeStamp};
use aura_journal::extensibility::DomainFact;
use aura_testkit::mock_effects::MockEffects;

fn test_context() -> ContextId {
    ContextId::from_uuid(uuid::Uuid::from_bytes([1u8; 16]))
}

fn test_channel() -> ChannelId {
    ChannelId::from_bytes([2u8; 32])
}

fn test_authority() -> AuthorityId {
    AuthorityId::from_uuid(uuid::Uuid::from_bytes([3u8; 16]))
}

// =============================================================================
// AmpRuntimeConfig Tests
// =============================================================================

#[test]
fn test_amp_runtime_config_defaults() {
    let config = AmpRuntimeConfig::default();

    assert_eq!(config.default_skip_window, 1024);
    assert_eq!(config.default_flow_cost, 1);
}

#[test]
fn test_amp_runtime_config_custom() {
    let config = AmpRuntimeConfig {
        default_skip_window: 2048,
        default_flow_cost: 5,
    };

    assert_eq!(config.default_skip_window, 2048);
    assert_eq!(config.default_flow_cost, 5);
}

#[test]
fn test_amp_runtime_config_clone() {
    let config1 = AmpRuntimeConfig {
        default_skip_window: 512,
        default_flow_cost: 10,
    };
    let config2 = config1.clone();

    assert_eq!(config1.default_skip_window, config2.default_skip_window);
    assert_eq!(config1.default_flow_cost, config2.default_flow_cost);
}

// =============================================================================
// ChannelMembershipFact Tests
// =============================================================================

#[test]
fn test_channel_membership_fact_joined() {
    let timestamp = TimeStamp::OrderClock(OrderTime([42u8; 32]));
    let fact = ChannelMembershipFact::new(
        test_context(),
        test_channel(),
        test_authority(),
        ChannelParticipantEvent::Joined,
        timestamp,
    );

    assert_eq!(fact.type_id(), "amp-channel-membership");
    assert_eq!(fact.context_id(), test_context());
}

#[test]
fn test_channel_membership_fact_left() {
    let timestamp = TimeStamp::OrderClock(OrderTime([0u8; 32]));
    let fact = ChannelMembershipFact::new(
        test_context(),
        test_channel(),
        test_authority(),
        ChannelParticipantEvent::Left,
        timestamp,
    );

    assert_eq!(fact.type_id(), "amp-channel-membership");
    assert_eq!(fact.context_id(), test_context());
}

#[test]
fn test_channel_membership_fact_serialization_roundtrip() {
    let timestamp = TimeStamp::OrderClock(OrderTime([99u8; 32]));
    let original = ChannelMembershipFact::new(
        test_context(),
        test_channel(),
        test_authority(),
        ChannelParticipantEvent::Joined,
        timestamp,
    );

    // Serialize to bytes
    let bytes = original.to_bytes();
    assert!(!bytes.is_empty(), "serialized bytes should not be empty");

    // Deserialize back
    let recovered = ChannelMembershipFact::from_bytes(&bytes).unwrap_or_else(|| {
        panic!("deserialization should succeed");
    });

    // Verify roundtrip
    assert_eq!(recovered.type_id(), original.type_id());
    assert_eq!(recovered.context_id(), original.context_id());
}

#[test]
fn test_channel_membership_fact_invalid_bytes() {
    let invalid_bytes = vec![0xFF, 0xFE, 0xFD];
    let result = ChannelMembershipFact::from_bytes(&invalid_bytes);
    assert!(result.is_none(), "should fail to deserialize invalid bytes");
}

#[test]
fn test_channel_participant_event_serialization() {
    // Test that events serialize correctly via JSON
    let joined = serde_json::to_string(&ChannelParticipantEvent::Joined).unwrap();
    let left = serde_json::to_string(&ChannelParticipantEvent::Left).unwrap();

    assert!(joined.contains("Joined"));
    assert!(left.contains("Left"));

    // Roundtrip
    let recovered_joined: ChannelParticipantEvent = serde_json::from_str(&joined).unwrap();
    let recovered_left: ChannelParticipantEvent = serde_json::from_str(&left).unwrap();

    assert!(matches!(recovered_joined, ChannelParticipantEvent::Joined));
    assert!(matches!(recovered_left, ChannelParticipantEvent::Left));
}

// =============================================================================
// MockEffects AmpChannelEffects Tests
// =============================================================================

#[tokio::test]
async fn test_mock_effects_create_channel() {
    let effects = MockEffects::deterministic();

    let params = ChannelCreateParams {
        context: test_context(),
        channel: Some(test_channel()),
        topic: None,
        skip_window: None,
    };

    let channel = effects.create_channel(params).await.unwrap();
    assert_eq!(channel, test_channel());
}

#[tokio::test]
async fn test_mock_effects_create_channel_auto_id() {
    let effects = MockEffects::deterministic();

    let params = ChannelCreateParams {
        context: test_context(),
        channel: None, // Auto-generate channel ID
        topic: None,
        skip_window: None,
    };

    let channel = effects.create_channel(params).await.unwrap();
    // Should generate a valid channel ID
    assert_ne!(channel, ChannelId::from_bytes([0u8; 32]));
}

#[tokio::test]
async fn test_mock_effects_join_channel() {
    let effects = MockEffects::deterministic();

    // First create a channel
    let create_params = ChannelCreateParams {
        context: test_context(),
        channel: Some(test_channel()),
        topic: None,
        skip_window: None,
    };
    effects.create_channel(create_params).await.unwrap();

    // Then join it
    let join_params = ChannelJoinParams {
        context: test_context(),
        channel: test_channel(),
        participant: test_authority(),
    };
    let result = effects.join_channel(join_params).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_mock_effects_join_nonexistent_channel() {
    let effects = MockEffects::deterministic();

    let join_params = ChannelJoinParams {
        context: test_context(),
        channel: test_channel(),
        participant: test_authority(),
    };

    let result = effects.join_channel(join_params).await;
    assert!(result.is_err(), "should fail to join nonexistent channel");
}

#[tokio::test]
async fn test_mock_effects_leave_channel() {
    let effects = MockEffects::deterministic();

    // Create and join channel first
    let create_params = ChannelCreateParams {
        context: test_context(),
        channel: Some(test_channel()),
        topic: None,
        skip_window: None,
    };
    effects.create_channel(create_params).await.unwrap();

    // Leave the channel
    let leave_params = ChannelLeaveParams {
        context: test_context(),
        channel: test_channel(),
        participant: test_authority(),
    };
    let result = effects.leave_channel(leave_params).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_mock_effects_close_channel() {
    let effects = MockEffects::deterministic();

    // Create channel first
    let create_params = ChannelCreateParams {
        context: test_context(),
        channel: Some(test_channel()),
        topic: None,
        skip_window: None,
    };
    effects.create_channel(create_params).await.unwrap();

    // Close the channel
    let close_params = ChannelCloseParams {
        context: test_context(),
        channel: test_channel(),
    };
    let result = effects.close_channel(close_params).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_mock_effects_close_nonexistent_channel() {
    let effects = MockEffects::deterministic();

    let close_params = ChannelCloseParams {
        context: test_context(),
        channel: test_channel(),
    };

    let result = effects.close_channel(close_params).await;
    assert!(result.is_err(), "should fail to close nonexistent channel");
}

#[tokio::test]
async fn test_mock_effects_send_message() {
    let effects = MockEffects::deterministic();

    // Create channel first
    let create_params = ChannelCreateParams {
        context: test_context(),
        channel: Some(test_channel()),
        topic: None,
        skip_window: None,
    };
    effects.create_channel(create_params).await.unwrap();

    // Send a message
    let send_params = ChannelSendParams {
        context: test_context(),
        channel: test_channel(),
        sender: test_authority(),
        plaintext: b"Hello, AMP!".to_vec(),
        reply_to: None,
    };

    let result = effects.send_message(send_params).await;
    assert!(result.is_ok());

    let ciphertext = result.unwrap();
    assert_eq!(ciphertext.header.context, test_context());
    assert_eq!(ciphertext.header.channel, test_channel());
    assert_eq!(ciphertext.header.chan_epoch, 0);
    assert_eq!(ciphertext.header.ratchet_gen, 0);
}

#[tokio::test]
async fn test_mock_effects_send_message_increments_gen() {
    let effects = MockEffects::deterministic();

    // Create channel
    let create_params = ChannelCreateParams {
        context: test_context(),
        channel: Some(test_channel()),
        topic: None,
        skip_window: None,
    };
    effects.create_channel(create_params).await.unwrap();

    // Send first message
    let send_params1 = ChannelSendParams {
        context: test_context(),
        channel: test_channel(),
        sender: test_authority(),
        plaintext: b"Message 1".to_vec(),
        reply_to: None,
    };
    let ct1 = effects.send_message(send_params1).await.unwrap();

    // Send second message
    let send_params2 = ChannelSendParams {
        context: test_context(),
        channel: test_channel(),
        sender: test_authority(),
        plaintext: b"Message 2".to_vec(),
        reply_to: None,
    };
    let ct2 = effects.send_message(send_params2).await.unwrap();

    // Generation should increment
    assert_eq!(ct1.header.ratchet_gen, 0);
    assert_eq!(ct2.header.ratchet_gen, 1);
}

#[tokio::test]
async fn test_mock_effects_send_message_on_closed_channel() {
    let effects = MockEffects::deterministic();

    // Create and close channel
    let create_params = ChannelCreateParams {
        context: test_context(),
        channel: Some(test_channel()),
        topic: None,
        skip_window: None,
    };
    effects.create_channel(create_params).await.unwrap();

    let close_params = ChannelCloseParams {
        context: test_context(),
        channel: test_channel(),
    };
    effects.close_channel(close_params).await.unwrap();

    // Try to send message on closed channel
    let send_params = ChannelSendParams {
        context: test_context(),
        channel: test_channel(),
        sender: test_authority(),
        plaintext: b"Should fail".to_vec(),
        reply_to: None,
    };

    let result = effects.send_message(send_params).await;
    assert!(result.is_err(), "should fail to send on closed channel");
}

#[tokio::test]
async fn test_mock_effects_close_increments_epoch() {
    let effects = MockEffects::deterministic();

    // Create channel
    let create_params = ChannelCreateParams {
        context: test_context(),
        channel: Some(test_channel()),
        topic: None,
        skip_window: None,
    };
    effects.create_channel(create_params).await.unwrap();

    // Get initial epoch via send
    let send_params = ChannelSendParams {
        context: test_context(),
        channel: test_channel(),
        sender: test_authority(),
        plaintext: b"Before close".to_vec(),
        reply_to: None,
    };
    let ct1 = effects.send_message(send_params).await.unwrap();
    let initial_epoch = ct1.header.chan_epoch;

    // Close channel (should increment epoch)
    let close_params = ChannelCloseParams {
        context: test_context(),
        channel: test_channel(),
    };
    effects.close_channel(close_params).await.unwrap();

    // Reopen channel (create again)
    let create_params2 = ChannelCreateParams {
        context: test_context(),
        channel: Some(test_channel()),
        topic: None,
        skip_window: None,
    };
    effects.create_channel(create_params2).await.unwrap();

    // Check epoch incremented
    let send_params2 = ChannelSendParams {
        context: test_context(),
        channel: test_channel(),
        sender: test_authority(),
        plaintext: b"After reopen".to_vec(),
        reply_to: None,
    };
    let ct2 = effects.send_message(send_params2).await.unwrap();

    assert_eq!(ct2.header.chan_epoch, initial_epoch + 1);
}

#[tokio::test]
async fn test_mock_effects_reset_clears_channels() {
    let effects = MockEffects::deterministic();

    // Create a channel
    let create_params = ChannelCreateParams {
        context: test_context(),
        channel: Some(test_channel()),
        topic: None,
        skip_window: None,
    };
    effects.create_channel(create_params).await.unwrap();

    // Reset effects
    effects.reset();

    // Try to join the channel - should fail because it was cleared
    let join_params = ChannelJoinParams {
        context: test_context(),
        channel: test_channel(),
        participant: test_authority(),
    };

    let result = effects.join_channel(join_params).await;
    assert!(result.is_err(), "channel should not exist after reset");
}
