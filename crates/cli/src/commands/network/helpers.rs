//! Test helper utilities
//!
//! Provides utility functions for creating test events and managing test data.

#![allow(dead_code)]

use aura_authentication::EventAuthorization;
use aura_crypto::Effects;
use aura_journal::{Event, EventType};
use aura_types::AccountId;

/// Create a DKD initiation event for testing
/// Create a DKD (Deterministic Key Derivation) event
pub fn create_dkd_event(
    effects: &Effects,
    account_id: AccountId,
    device_id: aura_types::DeviceId,
    nonce: u64,
) -> anyhow::Result<Event> {
    use aura_journal::{Event, InitiateDkdSessionEvent};
    use ed25519_dalek::Signature;
    use uuid::Uuid;

    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();

    Ok(Event {
        version: 1,
        event_id: aura_types::EventId(effects.gen_uuid()),
        account_id,
        timestamp: current_time + nonce, // Offset timestamp by nonce for uniqueness
        nonce,
        parent_hash: None,
        epoch_at_write: 1,
        event_type: EventType::InitiateDkdSession(InitiateDkdSessionEvent {
            session_id: Uuid::new_v4(),
            context_id: format!("test_context_{}", nonce).into_bytes(),
            threshold: 2,
            participants: vec![device_id],
            start_epoch: 1,
            ttl_in_epochs: 100,
        }),
        authorization: EventAuthorization::DeviceCertificate {
            device_id,
            signature: aura_crypto::Ed25519Signature(Signature::from_bytes(&[0u8; 64])), // Dummy signature for testing
        },
    })
}

/// Create an epoch tick event for testing
/// Create an epoch transition event
pub fn create_epoch_event(
    effects: &Effects,
    account_id: AccountId,
    device_id: aura_types::DeviceId,
    nonce: u64,
) -> anyhow::Result<Event> {
    use aura_journal::{EpochTickEvent, Event};
    use ed25519_dalek::Signature;

    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();

    Ok(Event {
        version: 1,
        event_id: aura_types::EventId(effects.gen_uuid()),
        account_id,
        timestamp: current_time + nonce,
        nonce,
        parent_hash: None,
        epoch_at_write: 1,
        event_type: EventType::EpochTick(EpochTickEvent {
            new_epoch: (nonce / 100) + 2, // Vary epoch based on nonce
            evidence_hash: [0u8; 32],
        }),
        authorization: EventAuthorization::DeviceCertificate {
            device_id,
            signature: aura_crypto::Ed25519Signature(Signature::from_bytes(&[0u8; 64])),
        },
    })
}

/// Create a device add event for testing
/// Create a device management event
pub fn create_device_event(
    effects: &Effects,
    account_id: AccountId,
    device_id: aura_types::DeviceId,
    nonce: u64,
) -> anyhow::Result<Event> {
    use aura_journal::{AddDeviceEvent, DeviceType, Event};
    use ed25519_dalek::{Signature, SigningKey};

    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();

    let signing_key = SigningKey::from_bytes(&[1u8; 32]);
    let public_key = signing_key.verifying_key();

    Ok(Event {
        version: 1,
        event_id: aura_types::EventId(effects.gen_uuid()),
        account_id,
        timestamp: current_time + nonce,
        nonce,
        parent_hash: None,
        epoch_at_write: 1,
        event_type: EventType::AddDevice(AddDeviceEvent {
            device_id,
            device_name: format!("Test Device {}", nonce),
            device_type: DeviceType::Native,
            public_key: public_key.to_bytes().to_vec(),
        }),
        authorization: EventAuthorization::DeviceCertificate {
            device_id,
            signature: aura_crypto::Ed25519Signature(Signature::from_bytes(&[0u8; 64])),
        },
    })
}

/// Create a conflict event for testing CRDT conflict resolution
/// Create a conflict resolution event
pub fn create_conflict_event(
    effects: &Effects,
    account_id: AccountId,
    device_id: aura_types::DeviceId,
    timestamp: u64,
    nonce: u64,
) -> anyhow::Result<Event> {
    use aura_journal::{EpochTickEvent, Event};
    use ed25519_dalek::Signature;

    Ok(Event {
        version: 1,
        event_id: aura_types::EventId(effects.gen_uuid()),
        account_id,
        timestamp,
        nonce,
        parent_hash: None,
        epoch_at_write: 1,
        event_type: EventType::EpochTick(EpochTickEvent {
            new_epoch: timestamp / 1000,      // Different epochs for conflicts
            evidence_hash: [nonce as u8; 32], // Different evidence for conflicts
        }),
        authorization: EventAuthorization::DeviceCertificate {
            device_id,
            signature: aura_crypto::Ed25519Signature(Signature::from_bytes(&[0u8; 64])),
        },
    })
}
