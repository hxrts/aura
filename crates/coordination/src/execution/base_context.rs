//! Base Protocol Context - Common execution environment for all protocols
//!
//! This module provides the base context that contains common fields and
//! functionality shared by all protocol-specific contexts.

use super::time::TimeSource;
use super::types::*;
use aura_crypto::Effects;
use aura_journal::{AccountLedger, Event, DeviceId};
use ed25519_dalek::SigningKey;
use rand::Rng;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Transport abstraction for protocol execution
/// 
/// This trait defines the minimal interface that coordination protocols
/// need from the transport layer. Transport implementations provide this.
#[async_trait::async_trait]
pub trait Transport: Send + Sync {
    /// Send a message to a peer
    async fn send_message(&self, peer_id: &str, message: &[u8]) -> Result<(), String>;
    
    /// Broadcast a message to all known peers
    async fn broadcast_message(&self, message: &[u8]) -> Result<(), String>;
    
    /// Check if a peer is reachable
    async fn is_peer_reachable(&self, peer_id: &str) -> bool;
}

/// Base context containing common fields for all protocols
pub struct BaseContext {
    /// Session/protocol ID
    pub session_id: Uuid,

    /// This device's ID
    pub device_id: Uuid,

    /// Device signing key for event authentication
    pub(crate) device_key: SigningKey,

    /// Participants in this protocol
    pub participants: Vec<DeviceId>,

    /// Threshold (if applicable)
    pub threshold: Option<usize>,

    /// CRDT ledger
    pub(crate) ledger: Arc<RwLock<AccountLedger>>,

    /// Network transport
    pub(crate) transport: Arc<dyn Transport>,

    /// Injectable effects (time, randomness)
    pub effects: Effects,

    /// Time source for cooperative yielding (simulation or production)
    pub(crate) time_source: Box<dyn TimeSource>,

    /// Pending events waiting to be processed
    pub(crate) pending_events: VecDeque<Event>,

    /// Events collected by await operations
    pub(crate) _collected_events: Vec<Event>,

    /// Index of last event we've read from the ledger
    pub(crate) last_read_event_index: usize,

    /// Device secret key for HPKE decryption
    pub device_secret: aura_crypto::HpkePrivateKey,
}

impl BaseContext {
    /// Create a new base context
    pub fn new(
        session_id: Uuid,
        device_id: Uuid,
        participants: Vec<DeviceId>,
        threshold: Option<usize>,
        ledger: Arc<RwLock<AccountLedger>>,
        transport: Arc<dyn Transport>,
        effects: Effects,
        device_key: SigningKey,
        time_source: Box<dyn TimeSource>,
    ) -> Self {
        // Generate a placeholder device secret using injected effects
        let mut rng = effects.rng();
        let device_keypair = aura_crypto::HpkeKeyPair::generate(&mut rng);

        // Register this context with the time source
        time_source.register_context(session_id);

        BaseContext {
            session_id,
            device_id,
            device_key,
            participants,
            threshold,
            ledger,
            transport,
            effects,
            time_source,
            pending_events: VecDeque::new(),
            _collected_events: Vec::new(),
            last_read_event_index: 0,
            device_secret: device_keypair.private_key,
        }
    }

    /// Sign an event with this device's key
    pub fn sign_event(&self, event: &Event) -> Result<ed25519_dalek::Signature, ProtocolError> {
        use ed25519_dalek::Signer;

        let event_hash = event.signable_hash().map_err(|e| ProtocolError {
            session_id: self.session_id,
            error_type: ProtocolErrorType::Other,
            message: format!("Failed to hash event for signing: {:?}", e),
        })?;

        Ok(self.device_key.sign(&event_hash))
    }

    /// Get key share (placeholder implementation)
    pub async fn get_key_share(&self) -> Result<Vec<u8>, ProtocolError> {
        // Placeholder: return dummy key share
        Ok(vec![0u8; 32])
    }

    /// Set key share (placeholder implementation)
    pub async fn set_key_share(&mut self, _share: Vec<u8>) -> Result<(), ProtocolError> {
        // Placeholder: would store the new share
        Ok(())
    }

    /// Get guardian share (placeholder implementation)
    pub async fn get_guardian_share(&self) -> Result<Vec<u8>, ProtocolError> {
        // Placeholder: return dummy guardian share
        Ok(vec![0u8; 32])
    }

    /// Generate nonce (placeholder implementation)
    pub async fn generate_nonce(&self) -> Result<u64, ProtocolError> {
        // Generate truly unique nonce by combining multiple sources of entropy
        let timestamp = self.effects.time.current_timestamp().unwrap_or(0);
        let device_hash = {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            use std::hash::Hasher;
            hasher.write(self.device_id.as_bytes());
            hasher.finish()
        };

        // Add random component for true uniqueness
        let mut rng = self.effects.rng();
        let random_component: u64 = rng.gen();

        // Combine all sources: timestamp + device_hash + random + session_id hash
        let session_hash = {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            use std::hash::Hasher;
            hasher.write(self.session_id.as_bytes());
            hasher.finish()
        };

        let unique_nonce = timestamp
            .wrapping_add(device_hash)
            .wrapping_add(random_component)
            .wrapping_add(session_hash);

        Ok(unique_nonce)
    }

    /// Get Merkle proof (placeholder implementation)
    pub async fn get_merkle_proof(&self) -> Result<Vec<u8>, ProtocolError> {
        // Placeholder: return dummy proof
        Ok(vec![0u8; 32])
    }

    /// Get guardian Merkle proof (placeholder implementation)
    pub async fn get_guardian_merkle_proof(
        &self,
        _guardian_id: aura_journal::GuardianId,
    ) -> Result<Vec<u8>, ProtocolError> {
        // Placeholder: return dummy proof
        Ok(vec![0u8; 32])
    }

    /// Get DKD commitment root (placeholder implementation)
    pub async fn get_dkd_commitment_root(&self) -> Result<[u8; 32], ProtocolError> {
        // Placeholder: return dummy root
        Ok([0u8; 32])
    }

    /// Get the HPKE public key for a specific device
    pub async fn get_device_public_key(
        &self,
        device_id: &DeviceId,
    ) -> Result<Vec<u8>, ProtocolError> {
        // For now, generate a deterministic key based on device ID
        // In production, this would fetch from the device metadata in the ledger
        use aura_crypto::Effects;

        // Create deterministic effects based on device ID
        let device_seed = device_id
            .0
            .as_bytes()
            .iter()
            .fold(0u64, |acc, &b| acc.wrapping_mul(31).wrapping_add(b as u64));
        let effects = Effects::deterministic(device_seed, 0);
        let mut rng = effects.rng();

        let keypair = aura_crypto::HpkeKeyPair::generate(&mut rng);
        Ok(keypair.public_key.to_bytes())
    }

    /// Get this device's HPKE private key
    pub async fn get_device_hpke_private_key(
        &self,
    ) -> Result<aura_crypto::HpkePrivateKey, ProtocolError> {
        // Generate the same deterministic key based on this device's ID
        // In production, this would be stored in secure device storage
        use aura_crypto::Effects;

        // Create deterministic effects based on device ID
        let device_seed = self
            .device_id
            .as_bytes()
            .iter()
            .fold(0u64, |acc, &b| acc.wrapping_mul(31).wrapping_add(b as u64));
        let effects = Effects::deterministic(device_seed, 0);
        let mut rng = effects.rng();

        let keypair = aura_crypto::HpkeKeyPair::generate(&mut rng);
        Ok(keypair.private_key)
    }

    /// Set context capsule (placeholder)
    pub fn set_context_capsule(
        &mut self,
        _capsule: std::collections::BTreeMap<String, String>,
    ) -> Result<(), ProtocolError> {
        // Placeholder: would store capsule for DKD
        // Using generic map to avoid circular dependency
        Ok(())
    }
}