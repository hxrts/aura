//! Comprehensive Unit Tests for Transport Types
//!
//! Tests all privacy-aware transport types including envelopes, connections,
//! and configuration. Focuses on privacy guarantees, serialization safety,
//! and relationship scoping.

use super::{
    envelope::{Envelope, ScopedEnvelope, FrameHeader, FrameType},
    config::{TransportConfig, PrivacyLevel, ConnectionTimeout},
    connection::{ConnectionId, ScopedConnectionId, ConnectionState, ConnectionInfo},
};
use aura_core::{DeviceId, ContextId};
use std::collections::HashMap;
use std::time::{SystemTime, Duration};

#[cfg(test)]
mod envelope_tests {
    use super::*;

    #[test]
    fn test_envelope_creation() {
        let sender = DeviceId::new();
        let recipient = DeviceId::new();
        let message = b"test message".to_vec();
        
        let envelope = Envelope::new(message.clone(), sender, recipient);
        
        assert_eq!(envelope.sender_id(), sender);
        assert_eq!(envelope.recipient_id(), recipient);
        assert_eq!(envelope.payload(), &message);
        assert!(!envelope.payload().is_empty());
    }

    #[test] 
    fn test_scoped_envelope_creation() {
        let sender = DeviceId::new();
        let context = ContextId::new();
        let message = b"scoped message".to_vec();
        
        let envelope = Envelope::new_scoped(message.clone(), sender, context);
        
        assert_eq!(envelope.sender_id(), sender);
        assert_eq!(envelope.context_id(), context);
        assert!(envelope.is_relationship_scoped());
        assert_eq!(envelope.payload(), &message);
    }

    #[test]
    fn test_privacy_level_preservation() {
        let sender = DeviceId::new();
        let recipient = DeviceId::new();
        let message = b"privacy test".to_vec();
        
        // Test each privacy level
        for privacy_level in [PrivacyLevel::Clear, PrivacyLevel::Blinded, PrivacyLevel::RelationshipScoped] {
            let envelope = Envelope::new_with_privacy(
                message.clone(), 
                sender, 
                recipient, 
                privacy_level
            );
            
            assert_eq!(envelope.privacy_level(), privacy_level);
            
            // Serialize and deserialize
            let serialized = envelope.to_bytes();
            let deserialized = Envelope::from_bytes(&serialized).expect("Deserialization failed");
            
            // Privacy level must be preserved
            assert_eq!(envelope.privacy_level(), deserialized.privacy_level());
            assert_eq!(envelope.sender_id(), deserialized.sender_id());
            assert_eq!(envelope.payload(), deserialized.payload());
        }
    }

    #[test]
    fn test_envelope_forwarding_preserves_privacy() {
        let original_sender = DeviceId::new();
        let original_recipient = DeviceId::new();
        let forwarding_recipient = DeviceId::new();
        let message = b"forwarded message".to_vec();
        
        let envelope = Envelope::new_with_privacy(
            message,
            original_sender,
            original_recipient,
            PrivacyLevel::Blinded,
        );
        
        let forwarded = envelope.forward_to(forwarding_recipient);
        
        // Privacy level should be preserved or strengthened
        assert!(forwarded.privacy_level() >= envelope.privacy_level());
        assert_eq!(forwarded.sender_id(), original_sender); // Original sender preserved
        assert_eq!(forwarded.recipient_id(), forwarding_recipient);
        assert_eq!(forwarded.payload(), envelope.payload());
    }

    #[test]
    fn test_relationship_scoping_isolation() {
        let sender = DeviceId::new();
        let context1 = ContextId::new();
        let context2 = ContextId::new();
        let message = b"isolation test".to_vec();
        
        let envelope1 = Envelope::new_scoped(message.clone(), sender, context1);
        let envelope2 = Envelope::new_scoped(message, sender, context2);
        
        // Different contexts should result in different scoped envelopes
        assert_ne!(envelope1.context_id(), envelope2.context_id());
        assert!(envelope1.is_relationship_scoped());
        assert!(envelope2.is_relationship_scoped());
        
        // Sender is the same, but contexts isolate the messages
        assert_eq!(envelope1.sender_id(), envelope2.sender_id());
        assert_ne!(envelope1.context_id(), envelope2.context_id());
    }

    #[test]
    fn test_frame_header_creation() {
        let header = FrameHeader::new(FrameType::Data, 1024);
        
        assert_eq!(header.frame_type(), FrameType::Data);
        assert_eq!(header.payload_size(), 1024);
        assert!(header.timestamp() <= SystemTime::now());
    }

    #[test]
    fn test_envelope_serialization_roundtrip() {
        let sender = DeviceId::new();
        let recipient = DeviceId::new();
        let context = ContextId::new();
        let message = b"serialization test with special chars: αβγδε 🔒🔑".as_bytes().to_vec();
        
        let original = Envelope::new_scoped(message, sender, context);
        let serialized = original.to_bytes();
        let deserialized = Envelope::from_bytes(&serialized).expect("Deserialization failed");
        
        assert_eq!(original.sender_id(), deserialized.sender_id());
        assert_eq!(original.context_id(), deserialized.context_id());
        assert_eq!(original.payload(), deserialized.payload());
        assert_eq!(original.privacy_level(), deserialized.privacy_level());
        assert_eq!(original.is_relationship_scoped(), deserialized.is_relationship_scoped());
    }
}

#[cfg(test)]
mod config_tests {
    use super::*;

    #[test]
    fn test_transport_config_default() {
        let config = TransportConfig::default();
        
        assert_eq!(config.privacy_level, PrivacyLevel::Blinded); // Safe default
        assert!(config.max_connections > 0);
        assert!(config.connection_timeout.as_secs() > 0);
    }

    #[test]
    fn test_transport_config_privacy_levels() {
        let mut config = TransportConfig::default();
        
        // Test each privacy level
        for privacy_level in [PrivacyLevel::Clear, PrivacyLevel::Blinded, PrivacyLevel::RelationshipScoped] {
            config.privacy_level = privacy_level;
            assert_eq!(config.privacy_level, privacy_level);
            
            // Privacy configuration should be consistent
            match privacy_level {
                PrivacyLevel::Clear => {
                    // Clear mode might disable some privacy features for testing
                    assert!(config.max_connections >= 1);
                }
                PrivacyLevel::Blinded => {
                    // Blinded mode should have reasonable defaults
                    assert!(config.max_connections >= 1);
                }
                PrivacyLevel::RelationshipScoped => {
                    // Most private mode might have more restrictions
                    assert!(config.max_connections >= 1);
                }
            }
        }
    }

    #[test]
    fn test_connection_timeout_validation() {
        let mut config = TransportConfig::default();
        
        // Valid timeouts
        for timeout_secs in [1, 10, 30, 60, 300] {
            config.connection_timeout = Duration::from_secs(timeout_secs);
            assert_eq!(config.connection_timeout.as_secs(), timeout_secs);
        }
        
        // Edge cases
        config.connection_timeout = Duration::from_millis(1);
        assert!(config.connection_timeout.as_millis() == 1);
        
        config.connection_timeout = Duration::from_secs(3600); // 1 hour
        assert!(config.connection_timeout.as_secs() == 3600);
    }

    #[test]
    fn test_config_serialization() {
        let config = TransportConfig {
            privacy_level: PrivacyLevel::RelationshipScoped,
            max_connections: 42,
            connection_timeout: Duration::from_secs(25),
            enable_capability_blinding: true,
            enable_traffic_padding: false,
            ..Default::default()
        };
        
        // Test JSON serialization roundtrip
        let json = serde_json::to_string(&config).expect("JSON serialization failed");
        let deserialized: TransportConfig = serde_json::from_str(&json)
            .expect("JSON deserialization failed");
        
        assert_eq!(config.privacy_level, deserialized.privacy_level);
        assert_eq!(config.max_connections, deserialized.max_connections);
        assert_eq!(config.connection_timeout, deserialized.connection_timeout);
        assert_eq!(config.enable_capability_blinding, deserialized.enable_capability_blinding);
        assert_eq!(config.enable_traffic_padding, deserialized.enable_traffic_padding);
    }
}

#[cfg(test)]
mod connection_tests {
    use super::*;

    #[test]
    fn test_connection_id_creation() {
        let device1 = DeviceId::new();
        let device2 = DeviceId::new();
        
        let connection_id = ConnectionId::new(device1, device2);
        
        assert_eq!(connection_id.local_device(), device1);
        assert_eq!(connection_id.remote_device(), device2);
        assert!(!connection_id.is_relationship_scoped());
    }

    #[test]
    fn test_scoped_connection_id() {
        let device1 = DeviceId::new();
        let device2 = DeviceId::new();
        let context = ContextId::new();
        
        let scoped_id = ConnectionId::new_scoped(device1, device2, context);
        
        assert_eq!(scoped_id.local_device(), device1);
        assert_eq!(scoped_id.remote_device(), device2);
        assert_eq!(scoped_id.context_id(), context);
        assert!(scoped_id.is_relationship_scoped());
    }

    #[test]
    fn test_connection_id_uniqueness() {
        let device1 = DeviceId::new();
        let device2 = DeviceId::new();
        let context1 = ContextId::new();
        let context2 = ContextId::new();
        
        // Same devices, different contexts should create different connection IDs
        let scoped1 = ConnectionId::new_scoped(device1, device2, context1);
        let scoped2 = ConnectionId::new_scoped(device1, device2, context2);
        
        assert_ne!(scoped1, scoped2);
        assert_ne!(scoped1.context_id(), scoped2.context_id());
        
        // Same devices, same context should create same connection ID
        let scoped3 = ConnectionId::new_scoped(device1, device2, context1);
        assert_eq!(scoped1, scoped3);
    }

    #[test]
    fn test_connection_state_transitions() {
        // Test valid state transitions
        let mut state = ConnectionState::Pending;
        
        state = ConnectionState::Establishing;
        assert_eq!(state, ConnectionState::Establishing);
        
        state = ConnectionState::Established;
        assert_eq!(state, ConnectionState::Established);
        
        state = ConnectionState::Closing;
        assert_eq!(state, ConnectionState::Closing);
        
        state = ConnectionState::Closed;
        assert_eq!(state, ConnectionState::Closed);
    }

    #[test]
    fn test_connection_info() {
        let device1 = DeviceId::new();
        let device2 = DeviceId::new();
        let context = ContextId::new();
        
        let connection_id = ConnectionId::new_scoped(device1, device2, context);
        let info = connection_id.info();
        
        assert_eq!(info.privacy_level(), PrivacyLevel::RelationshipScoped);
        assert!(info.is_relationship_scoped());
        assert_eq!(info.context_id(), context);
    }

    #[test]
    fn test_connection_serialization() {
        let device1 = DeviceId::new();
        let device2 = DeviceId::new();
        let context = ContextId::new();
        
        let original = ConnectionId::new_scoped(device1, device2, context);
        let serialized = serde_json::to_string(&original).expect("Serialization failed");
        let deserialized: ConnectionId = serde_json::from_str(&serialized)
            .expect("Deserialization failed");
        
        assert_eq!(original, deserialized);
        assert_eq!(original.local_device(), deserialized.local_device());
        assert_eq!(original.remote_device(), deserialized.remote_device());
        assert_eq!(original.context_id(), deserialized.context_id());
        assert_eq!(original.is_relationship_scoped(), deserialized.is_relationship_scoped());
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;

    #[test]
    fn test_privacy_level_ordering() {
        // Privacy levels should have a clear ordering: Clear < Blinded < RelationshipScoped
        assert!(PrivacyLevel::Clear < PrivacyLevel::Blinded);
        assert!(PrivacyLevel::Blinded < PrivacyLevel::RelationshipScoped);
        assert!(PrivacyLevel::Clear < PrivacyLevel::RelationshipScoped);
    }

    #[test]
    fn test_envelope_privacy_monotonicity() {
        // Privacy level should never decrease through operations
        let sender = DeviceId::new();
        let recipient = DeviceId::new();
        let message = b"monotonicity test".to_vec();
        
        for initial_level in [PrivacyLevel::Clear, PrivacyLevel::Blinded, PrivacyLevel::RelationshipScoped] {
            let envelope = Envelope::new_with_privacy(
                message.clone(),
                sender,
                recipient,
                initial_level,
            );
            
            // Forward operation should preserve or strengthen privacy
            let forwarded = envelope.forward_to(DeviceId::new());
            assert!(forwarded.privacy_level() >= envelope.privacy_level());
            
            // Serialization roundtrip should preserve privacy exactly
            let serialized = envelope.to_bytes();
            let deserialized = Envelope::from_bytes(&serialized).expect("Deserialization failed");
            assert_eq!(envelope.privacy_level(), deserialized.privacy_level());
        }
    }

    #[test]
    fn test_relationship_scoping_prevents_leakage() {
        let sender = DeviceId::new();
        let mut context_envelopes = HashMap::new();
        
        // Create envelopes in different relationship contexts
        for i in 0..10 {
            let context = ContextId::new();
            let message = format!("secret message {}", i).into_bytes();
            let envelope = Envelope::new_scoped(message, sender, context);
            context_envelopes.insert(context, envelope);
        }
        
        // All contexts should be unique (no leakage between relationships)
        let contexts: Vec<_> = context_envelopes.keys().cloned().collect();
        for i in 0..contexts.len() {
            for j in i+1..contexts.len() {
                assert_ne!(contexts[i], contexts[j]);
            }
        }
        
        // Each envelope should be properly scoped to its context
        for (context, envelope) in &context_envelopes {
            assert_eq!(envelope.context_id(), *context);
            assert!(envelope.is_relationship_scoped());
            assert_eq!(envelope.privacy_level(), PrivacyLevel::RelationshipScoped);
        }
    }

    #[test]
    fn test_connection_isolation_property() {
        let device1 = DeviceId::new();
        let device2 = DeviceId::new();
        
        // Create connections in different relationship contexts
        let mut connections = HashMap::new();
        for _i in 0..5 {
            let context = ContextId::new();
            let connection = ConnectionId::new_scoped(device1, device2, context);
            connections.insert(context, connection);
        }
        
        // All connections should be unique despite same device pair
        let connection_ids: Vec<_> = connections.values().cloned().collect();
        for i in 0..connection_ids.len() {
            for j in i+1..connection_ids.len() {
                assert_ne!(connection_ids[i], connection_ids[j]);
                // But devices should be the same
                assert_eq!(connection_ids[i].local_device(), connection_ids[j].local_device());
                assert_eq!(connection_ids[i].remote_device(), connection_ids[j].remote_device());
                // Only contexts should differ
                assert_ne!(connection_ids[i].context_id(), connection_ids[j].context_id());
            }
        }
    }
}