//! Integration Tests for Rendezvous System
//!
//! End-to-end tests for the fact-based rendezvous architecture:
//! - Descriptor publication and caching
//! - Transport selection and probing
//! - Handshake flow (initiator/responder)
//! - Channel establishment with epoch rotation
//! - Guard chain integration

#![allow(
    clippy::unwrap_used,
    clippy::disallowed_types,
    clippy::disallowed_methods
)] // Tests use unwrap for clarity; allow test-only hash utilities

use async_trait::async_trait;
use aura_core::effects::noise::{
    HandshakeState, NoiseEffects, NoiseError, NoiseParams, TransportState,
};
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::FlowCost;
use aura_rendezvous::{
    facts::{RendezvousDescriptor, RendezvousFact, TransportHint},
    new_channel::{ChannelManager, HandshakeConfig, Handshaker, SecureChannel},
    protocol::guards,
    service::{EffectCommand, GuardDecision, GuardSnapshot, RendezvousConfig, RendezvousService},
};

// =============================================================================
// Test Helpers
// =============================================================================

fn test_authority(seed: u8) -> AuthorityId {
    AuthorityId::new_from_entropy([seed; 32])
}

fn test_context(seed: u8) -> ContextId {
    ContextId::new_from_entropy([seed; 32])
}

fn test_snapshot(authority: AuthorityId, context: ContextId) -> GuardSnapshot {
    GuardSnapshot {
        authority_id: authority,
        context_id: context,
        flow_budget_remaining: FlowCost::new(1000),
        capabilities: vec![
            aura_guards::types::CapabilityId::from(guards::CAP_RENDEZVOUS_PUBLISH),
            aura_guards::types::CapabilityId::from(guards::CAP_RENDEZVOUS_CONNECT),
            aura_guards::types::CapabilityId::from(guards::CAP_RENDEZVOUS_RELAY),
        ],
        epoch: 1,
    }
}

fn test_descriptor(authority: AuthorityId, context: ContextId) -> RendezvousDescriptor {
    RendezvousDescriptor {
        authority_id: authority,
        context_id: context,
        transport_hints: vec![TransportHint::quic_direct("192.168.1.1:8443").unwrap()],
        handshake_psk_commitment: [42u8; 32],
        valid_from: 0,
        valid_until: 10_000,
        nonce: [0u8; 32],
        nickname_suggestion: None,
    }
}

struct MockNoise;
#[async_trait]
impl NoiseEffects for MockNoise {
    async fn create_handshake_state(
        &self,
        _params: NoiseParams,
    ) -> Result<HandshakeState, NoiseError> {
        Ok(HandshakeState(Box::new(())))
    }
    async fn write_message(
        &self,
        _state: HandshakeState,
        _payload: &[u8],
    ) -> Result<(Vec<u8>, HandshakeState), NoiseError> {
        Ok((vec![1, 2, 3], HandshakeState(Box::new(()))))
    }
    async fn read_message(
        &self,
        _state: HandshakeState,
        _message: &[u8],
    ) -> Result<(Vec<u8>, HandshakeState), NoiseError> {
        Ok((vec![], HandshakeState(Box::new(()))))
    }
    async fn into_transport_mode(
        &self,
        _state: HandshakeState,
    ) -> Result<TransportState, NoiseError> {
        Ok(TransportState(Box::new(())))
    }
    async fn encrypt_transport_message(
        &self,
        _state: &mut TransportState,
        payload: &[u8],
    ) -> Result<Vec<u8>, NoiseError> {
        Ok(payload.to_vec())
    }
    async fn decrypt_transport_message(
        &self,
        _state: &mut TransportState,
        message: &[u8],
    ) -> Result<Vec<u8>, NoiseError> {
        Ok(message.to_vec())
    }
}

// =============================================================================
// Descriptor Publication Tests
// =============================================================================

#[test]
fn test_descriptor_publication_flow() {
    // Setup: Alice wants to publish her descriptor
    let alice = test_authority(1);
    let context = test_context(100);

    let config = RendezvousConfig::default();
    let service = RendezvousService::new(alice, config);

    let snapshot = test_snapshot(alice, context);
    let hints = vec![TransportHint::quic_direct("10.0.0.1:8443").unwrap()];

    // Act: Prepare descriptor publication
    let outcome = service.prepare_publish_descriptor(&snapshot, context, hints, 1000);

    // Assert: Should be allowed with correct effects
    assert!(matches!(outcome.decision, GuardDecision::Allow));
    assert!(!outcome.effects.is_empty());

    // Verify flow budget charge is included
    let has_charge = outcome
        .effects
        .iter()
        .any(|e| matches!(e, EffectCommand::ChargeFlowBudget { .. }));
    assert!(has_charge, "Should include flow budget charge");

    // Verify journal append is included
    let has_append = outcome
        .effects
        .iter()
        .any(|e| matches!(e, EffectCommand::JournalAppend { .. }));
    assert!(has_append, "Should include journal append");
}

// =============================================================================
// Channel Establishment Tests
// =============================================================================

#[tokio::test]
async fn test_channel_establishment_flow() {
    let alice = test_authority(1);
    let bob = test_authority(2);
    let context = test_context(100);
    let psk = [42u8; 32];

    let config = RendezvousConfig::default();
    let mut service = RendezvousService::new(alice, config);

    let bob_descriptor = test_descriptor(bob, context);

    let snapshot = test_snapshot(alice, context);
    let mock_noise = MockNoise;

    // Act: Prepare channel establishment
    let outcome = service
        .prepare_establish_channel(
            &snapshot,
            context,
            bob,
            &psk,
            1000,
            &bob_descriptor,
            &mock_noise,
        )
        .await
        .unwrap();

    // Assert: Should be allowed
    assert!(matches!(outcome.decision, GuardDecision::Allow));

    // Should have handshake send effect
    let has_handshake = outcome
        .effects
        .iter()
        .any(|e| matches!(e, EffectCommand::SendHandshake { .. }));
    assert!(has_handshake, "Should include handshake send");
}

#[tokio::test]
async fn test_channel_establishment_requires_descriptor() {
    let alice = test_authority(1);
    let bob = test_authority(2);
    let context = test_context(100);
    let psk = [42u8; 32];

    let config = RendezvousConfig::default();
    let mut service = RendezvousService::new(alice, config);

    let snapshot = test_snapshot(alice, context);
    let other_context = test_context(101);
    let mismatched_descriptor = test_descriptor(bob, other_context);
    let mock_noise = MockNoise;

    let result = service
        .prepare_establish_channel(
            &snapshot,
            context,
            bob,
            &psk,
            1000,
            &mismatched_descriptor,
            &mock_noise,
        )
        .await;

    // Should fail - no descriptor
    assert!(result.is_err());
}

#[tokio::test]
async fn test_channel_establishment_rejects_expired_descriptor() {
    let alice = test_authority(1);
    let bob = test_authority(2);
    let context = test_context(100);
    let psk = [42u8; 32];

    let config = RendezvousConfig::default();
    let mut service = RendezvousService::new(alice, config);

    let snapshot = test_snapshot(alice, context);

    let mut expired_descriptor = test_descriptor(bob, context);
    expired_descriptor.valid_until = 900;
    let mock_noise = MockNoise;

    let result = service
        .prepare_establish_channel(
            &snapshot,
            context,
            bob,
            &psk,
            1000,
            &expired_descriptor,
            &mock_noise,
        )
        .await;

    assert!(result.is_err());
}

// =============================================================================
// Handshake Flow Tests
// =============================================================================

#[tokio::test]
async fn test_handshake_initiator_responder_flow() {
    let alice = test_authority(1);
    let bob = test_authority(2);
    let context = test_context(100);
    let psk = [42u8; 32];
    let epoch = 1u64;
    let mock_noise = MockNoise;

    // Alice initiates
    let alice_config = HandshakeConfig {
        local: alice,
        remote: bob,
        context_id: context,
        psk,
        timeout_ms: 5000,
    };
    let mut alice_handshaker = Handshaker::new(alice_config);

    // Bob responds
    let bob_config = HandshakeConfig {
        local: bob,
        remote: alice,
        context_id: context,
        psk,
        timeout_ms: 5000,
    };
    let mut bob_handshaker = Handshaker::new(bob_config);

    // Step 1: Alice creates init message
    let init_msg = alice_handshaker
        .create_init_message(epoch, &mock_noise)
        .await
        .unwrap();
    assert!(!init_msg.is_empty());

    // Step 2: Bob processes init
    bob_handshaker
        .process_init(&init_msg, epoch, &mock_noise)
        .await
        .unwrap();

    // Step 3: Bob creates response
    let response_msg = bob_handshaker
        .create_response(epoch, &mock_noise)
        .await
        .unwrap();
    assert!(!response_msg.is_empty());

    // Step 4: Alice processes response
    alice_handshaker
        .process_response(&response_msg, &mock_noise)
        .await
        .unwrap();

    // Step 5: Both complete
    let (alice_result, _) = alice_handshaker
        .complete(epoch, true, &mock_noise)
        .await
        .unwrap();
    let (bob_result, _) = bob_handshaker
        .complete(epoch, false, &mock_noise)
        .await
        .unwrap();

    // Channel IDs should match
    assert_eq!(alice_result.channel_id, bob_result.channel_id);
    assert!(alice_result.is_initiator);
    assert!(!bob_result.is_initiator);
}

#[tokio::test]
async fn test_handshake_psk_mismatch_detection() {
    let alice = test_authority(1);
    let bob = test_authority(2);
    let context = test_context(100);

    let config = RendezvousConfig::default();
    let mut service = RendezvousService::new(bob, config);

    // Bob's expected PSK
    let expected_psk = [42u8; 32];

    let snapshot = test_snapshot(bob, context);

    // Alice sends handshake with WRONG PSK
    let wrong_psk = [99u8; 32];
    let wrong_commitment = aura_core::hash::hash(&wrong_psk);

    let handshake = aura_rendezvous::protocol::NoiseHandshake {
        noise_message: vec![1, 2, 3],
        psk_commitment: wrong_commitment,
        epoch: 1,
    };
    let mock_noise = MockNoise;

    // Bob should reject (PSK commitment doesn't match)
    let (outcome, _) = service
        .prepare_handle_handshake(
            &snapshot,
            context,
            alice,
            handshake,
            &expected_psk,
            &mock_noise,
        )
        .await
        .unwrap();

    assert!(matches!(outcome.decision, GuardDecision::Deny { .. }));
}

// =============================================================================
// Channel Manager Tests
// =============================================================================

#[test]
fn test_channel_manager_lifecycle() {
    let alice = test_authority(1);
    let bob = test_authority(2);
    let context = test_context(100);

    let mut manager = ChannelManager::new();

    // No channel initially
    assert!(manager.find_by_context_peer(context, bob).is_none());

    // Create and register channel
    let channel_id = [77u8; 32];
    let mut channel = SecureChannel::new(channel_id, context, alice, bob, 1, None);
    channel.mark_active(); // Make it active

    assert!(channel.is_active());
    assert_eq!(channel.channel_id(), channel_id);
    assert_eq!(channel.epoch(), 1);

    manager.register(channel);

    // Should now be retrievable
    assert!(manager.find_by_context_peer(context, bob).is_some());

    // Get mutable and mark for rotation
    if let Some(ch) = manager.find_by_context_peer_mut(context, bob) {
        ch.mark_needs_rotation();
        assert!(ch.needs_rotation());
        ch.rotate(2).unwrap();
        assert!(!ch.needs_rotation());
        assert_eq!(ch.epoch(), 2);
    }
}

#[test]
fn test_channel_manager_epoch_advancement() {
    let alice = test_authority(1);
    let bob = test_authority(2);
    let carol = test_authority(3);
    let context = test_context(100);

    let mut manager = ChannelManager::new();

    // Create and register channels at epoch 1
    let mut ch1 = SecureChannel::new([1u8; 32], context, alice, bob, 1, None);
    ch1.mark_active();
    manager.register(ch1);

    let mut ch2 = SecureChannel::new([2u8; 32], context, alice, carol, 1, None);
    ch2.mark_active();
    manager.register(ch2);

    // Advance to epoch 2 - should mark all for rotation
    manager.advance_epoch(2);

    // Both should need rotation
    assert!(manager
        .find_by_context_peer(context, bob)
        .unwrap()
        .needs_rotation());
    assert!(manager
        .find_by_context_peer(context, carol)
        .unwrap()
        .needs_rotation());
}

// =============================================================================
// Guard Chain Integration Tests
// =============================================================================

#[test]
fn test_insufficient_flow_budget_blocks_publish() {
    let alice = test_authority(1);
    let context = test_context(100);

    let config = RendezvousConfig::default();
    let service = RendezvousService::new(alice, config);

    // Snapshot with insufficient budget
    let mut snapshot = test_snapshot(alice, context);
    snapshot.flow_budget_remaining = FlowCost::new(0); // No budget

    let hints = vec![TransportHint::quic_direct("10.0.0.1:8443").unwrap()];

    let outcome = service.prepare_publish_descriptor(&snapshot, context, hints, 1000);

    // Should be denied
    assert!(matches!(outcome.decision, GuardDecision::Deny { .. }));
    if let GuardDecision::Deny { reason } = outcome.decision {
        assert!(matches!(
            reason,
            aura_guards::types::GuardViolation::InsufficientFlowBudget { .. }
        ));
    }
}

#[tokio::test]
async fn test_missing_capability_blocks_connect() {
    let alice = test_authority(1);
    let bob = test_authority(2);
    let context = test_context(100);
    let psk = [42u8; 32];

    let config = RendezvousConfig::default();
    let mut service = RendezvousService::new(alice, config);
    let bob_descriptor = test_descriptor(bob, context);

    // Snapshot WITHOUT connect capability
    let mut snapshot = test_snapshot(alice, context);
    snapshot.capabilities = vec![aura_guards::types::CapabilityId::from(
        guards::CAP_RENDEZVOUS_PUBLISH,
    )]; // Only publish
    let mock_noise = MockNoise;

    let result = service
        .prepare_establish_channel(
            &snapshot,
            context,
            bob,
            &psk,
            1000,
            &bob_descriptor,
            &mock_noise,
        )
        .await;

    // Should be denied
    assert!(result.is_ok());
    let outcome = result.unwrap();
    assert!(matches!(outcome.decision, GuardDecision::Deny { .. }));
    if let GuardDecision::Deny { reason } = outcome.decision {
        assert!(matches!(
            reason,
            aura_guards::types::GuardViolation::MissingCapability { .. }
        ));
    }
}

// =============================================================================
// End-to-End Flow Tests
// =============================================================================

#[tokio::test]
async fn test_complete_discovery_to_channel_flow() {
    // Setup: Alice and Bob in same context
    let alice = test_authority(1);
    let bob = test_authority(2);
    let context = test_context(100);
    let psk = [42u8; 32];
    let epoch = 1u64;
    let mock_noise = MockNoise;

    // Both create services
    let config = RendezvousConfig::default();
    let alice_service = RendezvousService::new(alice, config.clone());
    let bob_service = RendezvousService::new(bob, config);

    // Step 1: Bob publishes his descriptor
    let bob_snapshot = test_snapshot(bob, context);
    let bob_hints = vec![TransportHint::quic_direct("10.0.0.2:8443").unwrap()];
    let publish_outcome =
        bob_service.prepare_publish_descriptor(&bob_snapshot, context, bob_hints, 1000);
    assert!(matches!(publish_outcome.decision, GuardDecision::Allow));

    // Step 2: Alice receives Bob's descriptor (simulated journal sync)
    let bob_descriptor = test_descriptor(bob, context);

    // Step 3: Alice initiates channel establishment
    // Requires mutable service for Alice
    let mut alice_service = alice_service; // rebind mut
    let alice_snapshot = test_snapshot(alice, context);
    let establish_outcome = alice_service
        .prepare_establish_channel(
            &alice_snapshot,
            context,
            bob,
            &psk,
            1000,
            &bob_descriptor,
            &mock_noise,
        )
        .await
        .unwrap();
    assert!(matches!(establish_outcome.decision, GuardDecision::Allow));

    // Step 4: Complete handshake (simulated message exchange)
    let alice_hs_config = HandshakeConfig {
        local: alice,
        remote: bob,
        context_id: context,
        psk,
        timeout_ms: 5000,
    };
    let mut alice_handshaker = Handshaker::new(alice_hs_config);

    let bob_hs_config = HandshakeConfig {
        local: bob,
        remote: alice,
        context_id: context,
        psk,
        timeout_ms: 5000,
    };
    let mut bob_handshaker = Handshaker::new(bob_hs_config);

    let init = alice_handshaker
        .create_init_message(epoch, &mock_noise)
        .await
        .unwrap();
    bob_handshaker
        .process_init(&init, epoch, &mock_noise)
        .await
        .unwrap();
    let response = bob_handshaker
        .create_response(epoch, &mock_noise)
        .await
        .unwrap();
    alice_handshaker
        .process_response(&response, &mock_noise)
        .await
        .unwrap();
    let (alice_result, _) = alice_handshaker
        .complete(epoch, true, &mock_noise)
        .await
        .unwrap();
    let (bob_result, _) = bob_handshaker
        .complete(epoch, false, &mock_noise)
        .await
        .unwrap();

    // Step 5: Both have matching channels
    assert_eq!(alice_result.channel_id, bob_result.channel_id);

    // Step 6: Create channel managers and register channels
    let mut alice_channels = ChannelManager::new();
    let mut bob_channels = ChannelManager::new();

    let channel_id = alice_result.channel_id;

    let mut alice_ch = SecureChannel::new(channel_id, context, alice, bob, epoch, None);
    alice_ch.mark_active();
    alice_channels.register(alice_ch);

    let mut bob_ch = SecureChannel::new(channel_id, context, bob, alice, epoch, None);
    bob_ch.mark_active();
    bob_channels.register(bob_ch);

    // Both can retrieve their channels
    assert!(alice_channels.find_by_context_peer(context, bob).is_some());
    assert!(bob_channels.find_by_context_peer(context, alice).is_some());
}

// =============================================================================
// Transport Selection Tests
// =============================================================================

#[test]
fn test_transport_hint_serialization() {
    use aura_journal::DomainFact;

    let hint = TransportHint::quic_direct("192.168.1.1:8443").unwrap();

    let descriptor = RendezvousDescriptor {
        authority_id: test_authority(1),
        context_id: test_context(100),
        transport_hints: vec![hint.clone()],
        handshake_psk_commitment: [0u8; 32],
        valid_from: 0,
        valid_until: 10_000,
        nonce: [0u8; 32],
        nickname_suggestion: None,
    };

    let fact = RendezvousFact::Descriptor(descriptor.clone());

    // Serialize and deserialize
    let bytes = fact.to_bytes();
    let recovered = RendezvousFact::from_bytes(&bytes).unwrap();

    if let RendezvousFact::Descriptor(d) = recovered {
        assert_eq!(d.transport_hints, descriptor.transport_hints);
        assert_eq!(d.authority_id, descriptor.authority_id);
    } else {
        panic!("Expected Descriptor fact");
    }
}

#[test]
fn test_relay_transport_hint() {
    let relay = test_authority(99);
    let hint = TransportHint::websocket_relay(relay);

    let descriptor = RendezvousDescriptor {
        authority_id: test_authority(1),
        context_id: test_context(100),
        transport_hints: vec![hint],
        handshake_psk_commitment: [0u8; 32],
        valid_from: 0,
        valid_until: 10_000,
        nonce: [0u8; 32],
        nickname_suggestion: None,
    };

    // Should have relay hint
    assert!(descriptor.transport_hints.iter().any(|h| {
        matches!(
            h,
            TransportHint::WebSocketRelay {
                relay_authority,
            } if *relay_authority == relay
        )
    }));
}
