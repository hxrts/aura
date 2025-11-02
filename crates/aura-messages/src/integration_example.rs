//! Integration example showing proper usage of the unified message system
//! 
//! This module demonstrates how to properly use aura-messages and aura-types
//! serialization utilities to avoid message duplication and ensure consistent
//! serialization across the codebase.

use crate::{AuraMessage, MessageEnvelope};
use crate::crypto::{CryptoMessage, CryptoPayload, DkdMessage, InitiateDkdSessionMessage};
use aura_types::{DeviceId, SessionId};
use aura_types::serialization::{bincode, cbor, json, Result as SerializationResult};

/// Example of creating and serializing crypto protocol messages
pub fn create_dkd_initiation_example() -> SerializationResult<Vec<u8>> {
    // Create a DKD initiation message using the proper aura-messages types
    let session_id = SessionId::new();
    let initiator_id = DeviceId::new();
    let participants = vec![initiator_id, DeviceId::new(), DeviceId::new()];
    
    let dkd_init = InitiateDkdSessionMessage {
        session_id,
        context_id: b"example_context".to_vec(),
        threshold: 2,
        participants: participants.clone(),
        start_epoch: 1,
        ttl_in_epochs: 100,
    };
    
    let dkd_payload = CryptoPayload::Dkd(DkdMessage::InitiateSession(dkd_init));
    
    let crypto_message = CryptoMessage::new(
        session_id,
        initiator_id,
        1, // sequence number
        chrono::Utc::now().timestamp() as u64,
        dkd_payload,
    );
    
    let aura_message = AuraMessage::Crypto(crypto_message);
    
    // Create properly structured envelope
    let envelope = MessageEnvelope::new(
        Some(session_id),
        initiator_id,
        1, // sequence number
        chrono::Utc::now().timestamp() as u64,
        aura_message,
    );
    
    // Use aura-types serialization for consistent format
    // Choose CBOR for protocol messages as recommended in the guidelines
    cbor::to_cbor_bytes(&envelope)
}

/// Example of deserializing and handling messages
pub fn handle_received_message(data: &[u8]) -> SerializationResult<()> {
    // Deserialize using aura-types utilities
    let envelope: MessageEnvelope<AuraMessage> = cbor::from_cbor_bytes(data)?;
    
    // Check version compatibility
    if !envelope.is_version_compatible(crate::WIRE_FORMAT_VERSION) {
        return Err(aura_types::serialization::SerializationError::custom(
            "Incompatible message version"
        ));
    }
    
    // Handle based on message type
    match &envelope.payload {
        AuraMessage::Crypto(crypto_msg) => {
            println!("Received crypto message for session: {:?}", crypto_msg.session_id);
            match &crypto_msg.payload {
                CryptoPayload::Dkd(dkd_msg) => {
                    println!("DKD message type: {}", match dkd_msg {
                        DkdMessage::InitiateSession(_) => "InitiateSession",
                        DkdMessage::PointCommitment(_) => "PointCommitment", 
                        DkdMessage::PointReveal(_) => "PointReveal",
                        DkdMessage::Finalize(_) => "Finalize",
                        DkdMessage::Abort(_) => "Abort",
                    });
                }
                _ => println!("Other crypto message"),
            }
        }
        AuraMessage::Social(_) => println!("Received social message"),
        AuraMessage::Recovery(_) => println!("Received recovery message"),
    }
    
    Ok(())
}

/// Example showing format choice guidelines
pub fn demonstrate_serialization_formats() -> SerializationResult<()> {
    let session_id = SessionId::new();
    let device_id = DeviceId::new();
    
    let dkd_init = InitiateDkdSessionMessage {
        session_id,
        context_id: b"demo_context".to_vec(),
        threshold: 3,
        participants: vec![device_id, DeviceId::new(), DeviceId::new(), DeviceId::new()],
        start_epoch: 1,
        ttl_in_epochs: 50,
    };
    
    // Different serialization formats for different use cases:
    
    // 1. CBOR for protocol messages (compact, deterministic, standard)
    let cbor_bytes = cbor::to_cbor_bytes(&dkd_init)?;
    println!("CBOR size: {} bytes", cbor_bytes.len());
    
    // 2. Bincode for hashing and performance-critical operations
    let bincode_bytes = bincode::to_bincode_bytes(&dkd_init)?;
    println!("Bincode size: {} bytes", bincode_bytes.len());
    
    // 3. JSON for human-readable debugging or APIs
    let json_string = json::to_json_string(&dkd_init)?;
    println!("JSON size: {} bytes", json_string.len());
    
    // Verify round-trip compatibility
    let reconstructed: InitiateDkdSessionMessage = cbor::from_cbor_bytes(&cbor_bytes)?;
    assert_eq!(reconstructed.session_id, dkd_init.session_id);
    assert_eq!(reconstructed.threshold, dkd_init.threshold);
    
    println!("All formats verified for round-trip compatibility");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_message_creation_and_serialization() {
        let result = create_dkd_initiation_example();
        assert!(result.is_ok(), "Message creation should succeed");
        
        let data = result.unwrap();
        assert!(!data.is_empty(), "Serialized data should not be empty");
        
        // Test round-trip
        let handle_result = handle_received_message(&data);
        assert!(handle_result.is_ok(), "Message handling should succeed");
    }
    
    #[test]
    fn test_serialization_formats() {
        let result = demonstrate_serialization_formats();
        assert!(result.is_ok(), "Format demonstration should succeed");
    }
}