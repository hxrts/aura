//! Comprehensive Tests for Transport Coordination Protocols
//!
//! Tests Layer 4 choreographic coordination protocols including receipt
//! verification, secure channels, WebSocket handshakes, and channel management.
//! Focuses on session type safety and multi-party coordination.

use super::{
    choreography::{
        ChannelEstablishmentCoordinator, ChoreographicConfig, ChoreographicError,
        ReceiptCoordinationProtocol, WebSocketHandshakeCoordinator,
    },
    coordination::TransportCoordinator,
    receipt_verification::ReceiptVerificationCoordinator,
    secure_channel::SecureChannelCoordinator,
    TransportCoordinationConfig, TransportCoordinationError,
};
use aura_core::{ContextId, identifiers::DeviceId};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};
use tokio::time::timeout;

#[cfg(test)]
mod legacy_transport_types {
    use super::*;

    #[derive(Debug, Clone)]
    pub struct ReceiptData {
        pub receipt_id: String,
        pub sender_id: DeviceId,
        pub recipient_id: DeviceId,
        pub message_hash: Vec<u8>,
        pub signature: Vec<u8>,
        pub timestamp: SystemTime,
        pub context_id: ContextId,
    }

    #[derive(Debug, Clone)]
    pub enum VerificationOutcome {
        Valid { confidence: u8 },
        Invalid { reason: String },
    }

    #[derive(Debug, Clone)]
    pub struct ReceiptVerificationResponse {
        pub verification_id: String,
        pub verifier_id: DeviceId,
        pub verification_result: VerificationOutcome,
        pub verification_proof: Vec<u8>,
        pub anti_replay_token: Vec<u8>,
        pub timestamp: SystemTime,
    }

    #[derive(Debug, Clone)]
    pub enum WebSocketHandshakeResult {
        Success,
        ProtocolMismatch { reason: String },
        CapabilityDenied { missing_capabilities: Vec<String> },
    }

    #[derive(Debug, Clone)]
    pub struct WebSocketHandshakeResponse {
        pub session_id: String,
        pub responder_id: DeviceId,
        pub accepted_protocols: Vec<String>,
        pub granted_capabilities: Vec<String>,
        pub handshake_result: WebSocketHandshakeResult,
    }

    #[derive(Debug, Clone)]
    pub enum ChannelType {
        SecureMessaging,
        FileTransfer,
        StreamingData,
        Control,
    }

    #[derive(Debug, Clone)]
    pub struct ResourceRequirements {
        pub bandwidth_mbps: u64,
        pub storage_mb: u64,
        pub cpu_cores: u64,
        pub memory_mb: u64,
    }

    #[derive(Debug, Clone)]
    pub struct AllocatedResources {
        pub bandwidth_allocated: u64,
        pub storage_allocated: u64,
        pub cpu_allocated: u64,
        pub memory_allocated: u64,
    }

    #[derive(Debug, Clone)]
    pub enum ConfirmationResult {
        Confirmed,
        InsufficientResources { missing: ResourceRequirements },
        CapabilityDenied { required: Vec<String> },
    }

    #[derive(Debug, Clone)]
    pub struct ChannelConfirmation {
        pub channel_id: String,
        pub participant_id: DeviceId,
        pub confirmation_result: ConfirmationResult,
        pub allocated_resources: AllocatedResources,
        pub timestamp: SystemTime,
    }
}

#[cfg(test)]
mod coordination_tests {
    use super::*;

    #[test]
    fn test_transport_coordinator_creation() {
        let config = TransportCoordinationConfig::default();
        let coordinator = TransportCoordinator::new(config);

        assert_eq!(coordinator.active_connections(), 0);
        assert!(!coordinator.is_at_capacity());
    }

    #[test]
    fn test_coordination_config_validation() {
        let config = TransportCoordinationConfig {
            max_connections: 0, // Invalid
            connection_timeout: Duration::from_secs(30),
            max_retries: 3,
            default_capabilities: vec!["transport".to_string()],
        };

        let result = TransportCoordinator::validate_config(&config);
        assert!(result.is_err());

        let valid_config = TransportCoordinationConfig {
            max_connections: 10,
            ..config
        };

        let result = TransportCoordinator::validate_config(&valid_config);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_local_coordination() {
        let config = TransportCoordinationConfig::default();
        let mut coordinator = TransportCoordinator::new(config);

        let peer_id = DeviceId::new();
        let connection_id = "test-connection".to_string();

        // Test local connection registration (no choreography)
        let result = coordinator
            .register_connection(connection_id.clone(), peer_id)
            .await;
        assert!(result.is_ok());
        assert_eq!(coordinator.active_connections(), 1);

        // Test connection lookup
        let found = coordinator.find_connection(&connection_id);
        assert!(found.is_some());
        assert_eq!(found.unwrap().peer_id, peer_id);

        // Test connection cleanup
        coordinator.cleanup_connection(&connection_id).await;
        assert_eq!(coordinator.active_connections(), 0);
    }

    #[tokio::test]
    async fn test_connection_capacity_limits() {
        let config = TransportCoordinationConfig {
            max_connections: 2,
            ..Default::default()
        };
        let mut coordinator = TransportCoordinator::new(config);

        // Add connections up to capacity
        let result1 = coordinator
            .register_connection("conn1".to_string(), DeviceId::new())
            .await;
        assert!(result1.is_ok());

        let result2 = coordinator
            .register_connection("conn2".to_string(), DeviceId::new())
            .await;
        assert!(result2.is_ok());

        assert!(coordinator.is_at_capacity());

        // Adding another should fail
        let result3 = coordinator
            .register_connection("conn3".to_string(), DeviceId::new())
            .await;
        assert!(result3.is_err());
    }

    #[tokio::test]
    async fn test_connection_timeout_handling() {
        let config = TransportCoordinationConfig {
            connection_timeout: Duration::from_millis(100),
            ..Default::default()
        };
        let mut coordinator = TransportCoordinator::new(config);

        // Register a connection
        let connection_id = "timeout-test".to_string();
        coordinator
            .register_connection(connection_id.clone(), DeviceId::new())
            .await
            .unwrap();

        // Wait for timeout
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Run cleanup
        coordinator.cleanup_stale_connections().await;

        // Connection should be cleaned up
        assert_eq!(coordinator.active_connections(), 0);
    }
}

#[cfg(test)]
mod receipt_verification_tests {
    use super::legacy_transport_types::{
        ReceiptData, ReceiptVerificationResponse, VerificationOutcome,
    };
    use super::*;

    #[test]
    fn test_receipt_coordinator_creation() {
        let config = ChoreographicConfig::default();
        let coordinator = ReceiptVerificationCoordinator::new(DeviceId::new(), config);

        assert_eq!(coordinator.active_verifications(), 0);
    }

    #[tokio::test]
    async fn test_receipt_verification_workflow() {
        let coordinator_id = DeviceId::new();
        let verifier1_id = DeviceId::new();
        let verifier2_id = DeviceId::new();
        let context_id = ContextId::new("test_context");

        let config = ChoreographicConfig::default();
        let mut coordinator = ReceiptVerificationCoordinator::new(coordinator_id, config);

        // Create test receipt data
        let receipt_data = ReceiptData {
            receipt_id: "test-receipt".to_string(),
            sender_id: DeviceId::new(),
            recipient_id: DeviceId::new(),
            message_hash: vec![0x01, 0x02, 0x03, 0x04],
            signature: vec![0xAA, 0xBB, 0xCC, 0xDD],
            timestamp: SystemTime::now(),
            context_id,
        };

        // Initiate verification (choreographic protocol)
        let verification_id = coordinator
            .initiate_verification(receipt_data, vec![verifier1_id, verifier2_id])
            .expect("Verification initiation failed");

        assert_eq!(coordinator.active_verifications(), 1);
        assert!(!verification_id.is_empty());
    }

    #[tokio::test]
    async fn test_verification_response_processing() {
        let coordinator_id = DeviceId::new();
        let verifier_id = DeviceId::new();

        let config = ChoreographicConfig::default();
        let mut coordinator = ReceiptVerificationCoordinator::new(coordinator_id, config);

        // First initiate a verification
        let receipt_data = ReceiptData {
            receipt_id: "response-test".to_string(),
            sender_id: DeviceId::new(),
            recipient_id: DeviceId::new(),
            message_hash: vec![0x05, 0x06, 0x07, 0x08],
            signature: vec![0xEE, 0xFF, 0x00, 0x11],
            timestamp: SystemTime::now(),
            context_id: ContextId::new("test_context"),
        };

        let verification_id = coordinator
            .initiate_verification(receipt_data, vec![verifier_id])
            .unwrap();

        // Create verification response
        let response = ReceiptVerificationResponse {
            verification_id: verification_id.clone(),
            verifier_id,
            verification_result: VerificationOutcome::Valid { confidence: 95 },
            verification_proof: vec![0x11, 0x22, 0x33],
            anti_replay_token: vec![0x44, 0x55, 0x66],
            timestamp: SystemTime::now(),
        };

        // Process response (choreographic coordination)
        let result = coordinator.process_verification_response(response);
        assert!(result.is_ok());

        // Build consensus
        let consensus = coordinator.build_consensus(&verification_id);
        assert!(consensus.is_ok());

        // Should indicate successful verification
        match consensus.unwrap() {
            aura_transport::protocols::websocket::ConsensusResult::Valid { confirmation_count } => {
                assert_eq!(confirmation_count, 1);
            }
            _ => panic!("Expected valid consensus result"),
        }
    }

    #[tokio::test]
    async fn test_anti_replay_protection() {
        let coordinator_id = DeviceId::new();
        let config = ChoreographicConfig::default();
        let mut coordinator = ReceiptVerificationCoordinator::new(coordinator_id, config);

        let receipt_data = ReceiptData {
            receipt_id: "replay-test".to_string(),
            sender_id: DeviceId::new(),
            recipient_id: DeviceId::new(),
            message_hash: vec![0xDE, 0xAD, 0xBE, 0xEF], // Same hash for replay test
            signature: vec![0x12, 0x34, 0x56, 0x78],
            timestamp: SystemTime::now(),
            context_id: ContextId::new("test_context"),
        };

        // First verification should succeed
        let result1 =
            coordinator.initiate_verification(receipt_data.clone(), vec![DeviceId::new()]);
        assert!(result1.is_ok());

        // Second verification with same message hash should fail (replay protection)
        let result2 = coordinator.initiate_verification(receipt_data, vec![DeviceId::new()]);
        assert!(result2.is_err());

        // Clean up replay prevention cache
        coordinator.cleanup_replay_prevention(Duration::from_secs(0)); // Force cleanup

        // Now it should work again
        let receipt_data_new = ReceiptData {
            receipt_id: "replay-test-2".to_string(),
            sender_id: DeviceId::new(),
            recipient_id: DeviceId::new(),
            message_hash: vec![0xDE, 0xAD, 0xBE, 0xEF], // Same hash but after cleanup
            signature: vec![0x12, 0x34, 0x56, 0x78],
            timestamp: SystemTime::now(),
            context_id: ContextId::new("test_context"),
        };

        let result3 = coordinator.initiate_verification(receipt_data_new, vec![DeviceId::new()]);
        assert!(result3.is_ok());
    }
}

#[cfg(test)]
mod websocket_choreography_tests {
    use super::legacy_transport_types::{WebSocketHandshakeResponse, WebSocketHandshakeResult};
    use super::*;

    #[test]
    fn test_websocket_handshake_coordinator() {
        let device_id = DeviceId::new();
        let config = ChoreographicConfig::default();

        let coordinator = WebSocketHandshakeCoordinator::new(device_id, config);
        assert_eq!(coordinator.active_handshakes(), 0);
    }

    #[tokio::test]
    async fn test_websocket_handshake_initiation() {
        let initiator_id = DeviceId::new();
        let peer_id = DeviceId::new();
        let context_id = ContextId::new("test_context");

        let config = ChoreographicConfig::default();
        let mut coordinator = WebSocketHandshakeCoordinator::new(initiator_id, config);

        // Initiate choreographic handshake
        let session_id = coordinator
            .initiate_handshake(
                peer_id,
                "ws://test.example.com:8080/socket".to_string(),
                context_id,
            )
            .expect("Handshake initiation failed");

        assert_eq!(coordinator.active_handshakes(), 1);
        assert!(!session_id.is_empty());

        let state = coordinator.get_handshake_state(&session_id);
        assert!(state.is_some());
    }

    #[tokio::test]
    async fn test_websocket_handshake_response_processing() {
        let initiator_id = DeviceId::new();
        let responder_id = DeviceId::new();
        let context_id = ContextId::new("test_context");

        let config = ChoreographicConfig::default();
        let mut coordinator = WebSocketHandshakeCoordinator::new(initiator_id, config);

        // Start handshake
        let session_id = coordinator
            .initiate_handshake(
                responder_id,
                "ws://test.example.com:8080/socket".to_string(),
                context_id,
            )
            .unwrap();

        // Create successful response
        let response = WebSocketHandshakeResponse {
            session_id: session_id.clone(),
            responder_id,
            accepted_protocols: vec!["aura-v1".to_string()],
            granted_capabilities: vec!["messaging".to_string()],
            handshake_result: WebSocketHandshakeResult::Success,
        };

        // Process response (choreographic coordination)
        let result = coordinator.process_handshake_response(&response);
        assert!(result.is_ok());
        assert!(result.unwrap()); // Should indicate success

        // Verify handshake completed
        let state = coordinator.get_handshake_state(&session_id);
        assert!(state.is_some());
    }

    #[tokio::test]
    async fn test_websocket_handshake_failure_scenarios() {
        let initiator_id = DeviceId::new();
        let responder_id = DeviceId::new();

        let config = ChoreographicConfig::default();
        let mut coordinator = WebSocketHandshakeCoordinator::new(initiator_id, config);

        let session_id = coordinator
            .initiate_handshake(
                responder_id,
                "ws://test.example.com/socket".to_string(),
                ContextId::new("test_context"),
            )
            .unwrap();

        // Test protocol mismatch failure
        let mismatch_response = WebSocketHandshakeResponse {
            session_id: session_id.clone(),
            responder_id,
            accepted_protocols: vec![],
            granted_capabilities: vec![],
            handshake_result: WebSocketHandshakeResult::ProtocolMismatch {
                supported: vec!["incompatible-protocol".to_string()],
            },
        };

        let result = coordinator.process_handshake_response(&mismatch_response);
        assert!(result.is_ok());
        assert!(!result.unwrap()); // Should indicate failure

        // Test capability denial
        let capability_denial_response = WebSocketHandshakeResponse {
            session_id: session_id.clone(),
            responder_id,
            accepted_protocols: vec![],
            granted_capabilities: vec![],
            handshake_result: WebSocketHandshakeResult::CapabilityDenied {
                missing: vec!["required_capability".to_string()],
            },
        };

        let result = coordinator.process_handshake_response(&capability_denial_response);
        assert!(result.is_ok());
        assert!(!result.unwrap()); // Should indicate failure
    }

    #[tokio::test]
    async fn test_websocket_concurrent_handshakes() {
        let initiator_id = DeviceId::new();
        let config = ChoreographicConfig {
            max_concurrent_protocols: 2,
            ..Default::default()
        };
        let mut coordinator = WebSocketHandshakeCoordinator::new(initiator_id, config);

        // Start multiple handshakes up to limit
        let session1 = coordinator.initiate_handshake(
            DeviceId::new(),
            "ws://peer1.example.com/socket".to_string(),
            ContextId::new("test_context"),
        );
        assert!(session1.is_ok());

        let session2 = coordinator.initiate_handshake(
            DeviceId::new(),
            "ws://peer2.example.com/socket".to_string(),
            ContextId::new("test_context"),
        );
        assert!(session2.is_ok());

        assert_eq!(coordinator.active_handshakes(), 2);

        // Third handshake should fail (over limit)
        let session3 = coordinator.initiate_handshake(
            DeviceId::new(),
            "ws://peer3.example.com/socket".to_string(),
            ContextId::new("test_context"),
        );
        assert!(session3.is_err());

        // Clean up completed handshakes to make room
        let cleaned = coordinator.cleanup_completed();
        assert_eq!(cleaned, 0); // Nothing completed yet
    }
}

#[cfg(test)]
mod channel_management_tests {
    use super::legacy_transport_types::{
        AllocatedResources, ChannelConfirmation, ChannelType, ConfirmationResult,
        ResourceRequirements,
    };
    use super::*;

    #[test]
    fn test_channel_establishment_coordinator() {
        let device_id = DeviceId::new();
        let config = ChoreographicConfig::default();

        let coordinator = ChannelEstablishmentCoordinator::new(device_id, config);
        assert_eq!(coordinator.active_establishments(), 0);
    }

    #[tokio::test]
    async fn test_channel_establishment_workflow() {
        let coordinator_id = DeviceId::new();
        let participant1 = DeviceId::new();
        let participant2 = DeviceId::new();
        let context_id = ContextId::new("test_context");

        let config = ChoreographicConfig::default();
        let mut coordinator = ChannelEstablishmentCoordinator::new(coordinator_id, config);

        // Initiate channel establishment (choreographic protocol)
        let channel_id = coordinator
            .initiate_establishment(
                vec![participant1, participant2],
                ChannelType::SecureMessaging,
                context_id,
            )
            .expect("Channel establishment failed");

        assert_eq!(coordinator.active_establishments(), 1);
        assert!(!channel_id.is_empty());

        let status = coordinator.get_establishment_status(&channel_id);
        assert!(status.is_some());
    }

    #[tokio::test]
    async fn test_channel_confirmation_processing() {
        let coordinator_id = DeviceId::new();
        let participant_id = DeviceId::new();
        let context_id = ContextId::new("test_context");

        let config = ChoreographicConfig::default();
        let mut coordinator = ChannelEstablishmentCoordinator::new(coordinator_id, config);

        // Start establishment
        let channel_id = coordinator
            .initiate_establishment(vec![participant_id], ChannelType::FileTransfer, context_id)
            .unwrap();

        // Create confirmation
        let confirmation = ChannelConfirmation {
            channel_id: channel_id.clone(),
            participant_id,
            confirmation_result: ConfirmationResult::Confirmed,
            allocated_resources: AllocatedResources {
                bandwidth_allocated: 100,
                storage_allocated: 1024,
                cpu_allocated: 1,
                memory_allocated: 512,
            },
            timestamp: SystemTime::now(),
        };

        // Process confirmation (choreographic coordination)
        let result = coordinator.process_confirmation(confirmation);
        assert!(result.is_ok());
        assert!(result.unwrap()); // Should indicate all participants confirmed
    }

    #[tokio::test]
    async fn test_channel_resource_allocation() {
        let coordinator_id = DeviceId::new();
        let participants = vec![DeviceId::new(), DeviceId::new(), DeviceId::new()];
        let context_id = ContextId::new("test_context");

        let config = ChoreographicConfig::default();
        let mut coordinator = ChannelEstablishmentCoordinator::new(coordinator_id, config);

        let channel_id = coordinator
            .initiate_establishment(participants.clone(), ChannelType::StreamingData, context_id)
            .unwrap();

        // Process confirmations from all participants with different resource allocations
        let resource_allocations = vec![
            AllocatedResources {
                bandwidth_allocated: 50,
                storage_allocated: 512,
                cpu_allocated: 1,
                memory_allocated: 256,
            },
            AllocatedResources {
                bandwidth_allocated: 100,
                storage_allocated: 1024,
                cpu_allocated: 2,
                memory_allocated: 512,
            },
            AllocatedResources {
                bandwidth_allocated: 75,
                storage_allocated: 768,
                cpu_allocated: 1,
                memory_allocated: 384,
            },
        ];

        let mut all_confirmed = false;
        for (i, participant) in participants.iter().enumerate() {
            let confirmation = ChannelConfirmation {
                channel_id: channel_id.clone(),
                participant_id: *participant,
                confirmation_result: ConfirmationResult::Confirmed,
                allocated_resources: resource_allocations[i].clone(),
                timestamp: SystemTime::now(),
            };

            let result = coordinator.process_confirmation(confirmation);
            assert!(result.is_ok());
            all_confirmed = result.unwrap();
        }

        // All participants should have confirmed
        assert!(all_confirmed);
    }

    #[tokio::test]
    async fn test_channel_establishment_failure_scenarios() {
        let coordinator_id = DeviceId::new();
        let participant_id = DeviceId::new();
        let context_id = ContextId::new("test_context");

        let config = ChoreographicConfig::default();
        let mut coordinator = ChannelEstablishmentCoordinator::new(coordinator_id, config);

        let channel_id = coordinator
            .initiate_establishment(vec![participant_id], ChannelType::Control, context_id)
            .unwrap();

        // Test resource shortage scenario
        let resource_shortage_confirmation = ChannelConfirmation {
            channel_id: channel_id.clone(),
            participant_id,
            confirmation_result: ConfirmationResult::InsufficientResources {
                missing: aura_transport::protocols::websocket::ResourceRequirements {
                    bandwidth_mbps: 200,
                    storage_mb: 2048,
                    cpu_cores: 4,
                    memory_mb: 1024,
                },
            },
            allocated_resources: AllocatedResources {
                bandwidth_allocated: 0,
                storage_allocated: 0,
                cpu_allocated: 0,
                memory_allocated: 0,
            },
            timestamp: SystemTime::now(),
        };

        let result = coordinator.process_confirmation(resource_shortage_confirmation);
        // Should handle gracefully (might succeed processing but indicate failure)
        assert!(result.is_ok());

        // Test capability denial scenario
        let capability_denial_confirmation = ChannelConfirmation {
            channel_id: channel_id.clone(),
            participant_id,
            confirmation_result: ConfirmationResult::CapabilityDenied {
                required: vec!["advanced_channel_management".to_string()],
            },
            allocated_resources: AllocatedResources {
                bandwidth_allocated: 0,
                storage_allocated: 0,
                cpu_allocated: 0,
                memory_allocated: 0,
            },
            timestamp: SystemTime::now(),
        };

        let result = coordinator.process_confirmation(capability_denial_confirmation);
        assert!(result.is_ok()); // Should handle gracefully
    }
}

#[cfg(test)]
mod choreographic_integration_tests {
    use super::*;

    #[tokio::test]
    async fn test_multiple_choreographic_protocols() {
        let device_id = DeviceId::new();
        let config = ChoreographicConfig {
            max_concurrent_protocols: 5,
            ..Default::default()
        };

        // Create multiple coordinators
        let ws_coordinator = WebSocketHandshakeCoordinator::new(device_id, config.clone());
        let channel_coordinator = ChannelEstablishmentCoordinator::new(device_id, config.clone());
        let receipt_coordinator = ReceiptVerificationCoordinator::new(device_id, config);

        // All should be able to coexist
        assert_eq!(ws_coordinator.active_handshakes(), 0);
        assert_eq!(channel_coordinator.active_establishments(), 0);
        assert_eq!(receipt_coordinator.active_verifications(), 0);
    }

    #[tokio::test]
    async fn test_choreographic_config_validation() {
        let configs = vec![
            ChoreographicConfig {
                max_concurrent_protocols: 0, // Invalid
                protocol_timeout: Duration::from_secs(30),
                required_capabilities: vec![],
                extension_registry: Default::default(),
            },
            ChoreographicConfig {
                max_concurrent_protocols: 10,
                protocol_timeout: Duration::from_millis(0), // Invalid
                required_capabilities: vec![],
                extension_registry: Default::default(),
            },
        ];

        for config in configs {
            let result = ChoreographicConfig::validate(&config);
            assert!(result.is_err(), "Config should be invalid: {:?}", config);
        }

        // Valid config
        let valid_config = ChoreographicConfig::default();
        let result = ChoreographicConfig::validate(&valid_config);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_session_type_safety() {
        // This is a conceptual test - in practice, session type safety
        // is enforced at compile time by the choreography system
        let device_id = DeviceId::new();
        let config = ChoreographicConfig::default();

        let mut ws_coordinator = WebSocketHandshakeCoordinator::new(device_id, config);

        // Initiate handshake (valid first step)
        let session_id = ws_coordinator.initiate_handshake(
            DeviceId::new(),
            "ws://test.example.com/socket".to_string(),
            ContextId::new("test_context"),
        );
        assert!(session_id.is_ok());

        // Try to process response for non-existent session (should fail gracefully)
        let invalid_response = WebSocketHandshakeResponse {
            session_id: "non-existent".to_string(),
            responder_id: DeviceId::new(),
            accepted_protocols: vec![],
            granted_capabilities: vec![],
            handshake_result: WebSocketHandshakeResult::Success,
        };

        let result = ws_coordinator.process_handshake_response(&invalid_response);
        assert!(result.is_err()); // Should reject invalid session
    }
}
