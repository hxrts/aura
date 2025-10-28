//! Base Protocol Context - Common execution environment for all protocols
//!
//! This module provides the base context that contains common fields and
//! functionality shared by all protocol-specific contexts.

use super::time::TimeSource;
use super::types::*;
use aura_crypto::Effects;
use aura_journal::{AccountLedger, Event};
use aura_types::{DeviceId, GuardianId};
use ed25519_dalek::SigningKey;
use rand::Rng;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

#[cfg(feature = "dev-console")]
use crate::instrumentation::InstrumentationHooks;

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

    /// Optional instrumentation hooks for dev console integration
    #[cfg(feature = "dev-console")]
    pub(crate) instrumentation: Option<InstrumentationHooks>,
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
            #[cfg(feature = "dev-console")]
            instrumentation: None,
        }
    }

    /// Get the device signing key
    pub fn device_key(&self) -> &SigningKey {
        &self.device_key
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

    /// Get key share (requires crypto service integration)
    pub async fn get_key_share(&self) -> Result<Vec<u8>, ProtocolError> {
        Err(ProtocolError {
            session_id: self.session_id,
            error_type: ProtocolErrorType::Other,
            message: "Key share access not implemented - requires CryptoService integration"
                .to_string(),
        })
    }

    /// Set key share (requires crypto service integration)
    pub async fn set_key_share(&mut self, _share: Vec<u8>) -> Result<(), ProtocolError> {
        Err(ProtocolError {
            session_id: self.session_id,
            error_type: ProtocolErrorType::Other,
            message: "Key share storage not implemented - requires SecureStorage integration"
                .to_string(),
        })
    }

    /// Get guardian share (requires crypto service integration)
    pub async fn get_guardian_share(&self) -> Result<Vec<u8>, ProtocolError> {
        Err(ProtocolError {
            session_id: self.session_id,
            error_type: ProtocolErrorType::Other,
            message: "Guardian share access not implemented - requires CryptoService integration"
                .to_string(),
        })
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

    /// Get Merkle proof for current session's DKD commitments
    pub async fn get_merkle_proof(&self) -> Result<Vec<u8>, ProtocolError> {
        use aura_crypto::merkle::build_commitment_tree;
        use serde_json;

        // Get all DKD commitment hashes from the ledger state
        let commitment_hashes = self.get_dkd_commitment_hashes().await?;

        if commitment_hashes.is_empty() {
            return Err(ProtocolError {
                session_id: self.session_id,
                error_type: ProtocolErrorType::Other,
                message: "No DKD commitments found for Merkle proof generation".to_string(),
            });
        }

        // Build Merkle tree from commitments
        let (merkle_root, proofs) =
            build_commitment_tree(&commitment_hashes).map_err(|e| ProtocolError {
                session_id: self.session_id,
                error_type: ProtocolErrorType::Other,
                message: format!("Failed to build Merkle tree: {}", e),
            })?;

        // For this session, find the relevant proof
        // In a real implementation, we'd determine which commitment corresponds to this session
        let session_proof_index = self.get_session_commitment_index().await?;

        if session_proof_index >= proofs.len() {
            return Err(ProtocolError {
                session_id: self.session_id,
                error_type: ProtocolErrorType::Other,
                message: "Session commitment index out of range".to_string(),
            });
        }

        let session_proof = &proofs[session_proof_index];

        // Serialize the proof with root for verification
        let proof_data = MerkleProofData {
            merkle_root,
            proof: session_proof.clone(),
            session_id: self.session_id,
        };

        serde_json::to_vec(&proof_data).map_err(|e| ProtocolError {
            session_id: self.session_id,
            error_type: ProtocolErrorType::Other,
            message: format!("Failed to serialize Merkle proof: {}", e),
        })
    }

    /// Get guardian Merkle proof for recovery verification
    pub async fn get_guardian_merkle_proof(
        &self,
        guardian_id: GuardianId,
    ) -> Result<Vec<u8>, ProtocolError> {
        use aura_crypto::merkle::build_commitment_tree;
        use serde_json;

        // Get guardian shares from the ledger for this account
        let guardian_share_hashes = self.get_guardian_share_hashes(guardian_id).await?;

        if guardian_share_hashes.is_empty() {
            return Err(ProtocolError {
                session_id: self.session_id,
                error_type: ProtocolErrorType::Other,
                message: format!("No guardian shares found for guardian {}", guardian_id.0),
            });
        }

        // Build Merkle tree from guardian share commitments
        let (merkle_root, proofs) =
            build_commitment_tree(&guardian_share_hashes).map_err(|e| ProtocolError {
                session_id: self.session_id,
                error_type: ProtocolErrorType::Other,
                message: format!("Failed to build guardian Merkle tree: {}", e),
            })?;

        // For recovery, we typically need proof for the most recent share
        let latest_share_index = guardian_share_hashes.len() - 1;
        let guardian_proof = &proofs[latest_share_index];

        // Serialize the guardian proof with additional metadata
        let proof_data = GuardianMerkleProofData {
            merkle_root,
            proof: guardian_proof.clone(),
            guardian_id,
            session_id: self.session_id,
            share_count: guardian_share_hashes.len(),
        };

        serde_json::to_vec(&proof_data).map_err(|e| ProtocolError {
            session_id: self.session_id,
            error_type: ProtocolErrorType::Other,
            message: format!("Failed to serialize guardian Merkle proof: {}", e),
        })
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

    /// Enable instrumentation with provided hooks
    #[cfg(feature = "dev-console")]
    pub fn enable_instrumentation(&mut self, hooks: InstrumentationHooks) {
        self.instrumentation = Some(hooks);
    }

    /// Disable instrumentation
    #[cfg(feature = "dev-console")]
    pub fn disable_instrumentation(&mut self) {
        self.instrumentation = None;
    }

    /// Check if instrumentation is enabled
    #[cfg(feature = "dev-console")]
    pub fn is_instrumentation_enabled(&self) -> bool {
        self.instrumentation.is_some()
    }

    /// Get instrumentation hooks if available
    #[cfg(feature = "dev-console")]
    pub fn instrumentation(&self) -> Option<&InstrumentationHooks> {
        self.instrumentation.as_ref()
    }

    /// Instrumentation hook for protocol start
    pub fn instrument_protocol_start(&self, protocol_name: &str) {
        #[cfg(feature = "dev-console")]
        if let Some(hooks) = &self.instrumentation {
            hooks.on_protocol_start(self.session_id, DeviceId(self.device_id), protocol_name);
        }
        #[cfg(not(feature = "dev-console"))]
        let _ = protocol_name;
    }

    /// Instrumentation hook for phase transitions
    pub fn instrument_phase_transition(
        &self,
        protocol_name: &str,
        from_phase: &str,
        to_phase: &str,
    ) {
        #[cfg(feature = "dev-console")]
        if let Some(hooks) = &self.instrumentation {
            hooks.on_phase_transition(
                self.session_id,
                DeviceId(self.device_id),
                protocol_name,
                from_phase,
                to_phase,
            );
        }
        #[cfg(not(feature = "dev-console"))]
        {
            let _ = (protocol_name, from_phase, to_phase);
        }
    }

    /// Instrumentation hook for event emission
    pub fn instrument_event_emit(&self, event_type: &str, event_size: usize) {
        #[cfg(feature = "dev-console")]
        if let Some(hooks) = &self.instrumentation {
            hooks.on_event_emit(
                self.session_id,
                DeviceId(self.device_id),
                event_type,
                event_size,
            );
        }
        #[cfg(not(feature = "dev-console"))]
        {
            let _ = (event_type, event_size);
        }
    }

    /// Instrumentation hook for event awaiting start
    pub fn instrument_event_await_start(&self, event_pattern: &str, threshold: Option<usize>) {
        #[cfg(feature = "dev-console")]
        if let Some(hooks) = &self.instrumentation {
            hooks.on_event_await_start(
                self.session_id,
                DeviceId(self.device_id),
                event_pattern,
                threshold,
            );
        }
        #[cfg(not(feature = "dev-console"))]
        {
            let _ = (event_pattern, threshold);
        }
    }

    /// Instrumentation hook for event awaiting completion
    pub fn instrument_event_await_complete(
        &self,
        event_pattern: &str,
        received_count: usize,
        success: bool,
    ) {
        #[cfg(feature = "dev-console")]
        if let Some(hooks) = &self.instrumentation {
            hooks.on_event_await_complete(
                self.session_id,
                DeviceId(self.device_id),
                event_pattern,
                received_count,
                success,
            );
        }
        #[cfg(not(feature = "dev-console"))]
        {
            let _ = (event_pattern, received_count, success);
        }
    }

    /// Instrumentation hook for protocol completion
    pub fn instrument_protocol_complete(
        &self,
        _protocol_name: &str,
        _success: bool,
        _result_summary: Option<serde_json::Value>,
    ) {
        #[cfg(feature = "dev-console")]
        if let Some(hooks) = &self.instrumentation {
            hooks.on_protocol_complete(
                self.session_id,
                DeviceId(self.device_id),
                _protocol_name,
                _success,
                _result_summary,
            );
        }
    }

    /// Instrumentation hook for protocol errors
    pub fn instrument_protocol_error(
        &self,
        _protocol_name: &str,
        _error_type: &str,
        _error_message: &str,
    ) {
        #[cfg(feature = "dev-console")]
        if let Some(hooks) = &self.instrumentation {
            hooks.on_protocol_error(
                self.session_id,
                DeviceId(self.device_id),
                _protocol_name,
                _error_type,
                _error_message,
            );
        }
    }

    // ========== Merkle Proof Helper Methods ==========

    /// Get DKD commitment hashes from ledger state for Merkle tree construction
    async fn get_dkd_commitment_hashes(&self) -> Result<Vec<[u8; 32]>, ProtocolError> {
        // In a real implementation, this would query the ledger state
        // For now, simulate with dummy commitment hashes based on session data
        use blake3;

        let commitment_count = 3; // Simulate 3 DKD commitments
        let mut commitments = Vec::new();

        for i in 0..commitment_count {
            // Generate deterministic commitment hash based on session and index
            let commitment_data = format!("{}:dkd_commitment:{}", self.session_id, i);
            let hash = blake3::hash(commitment_data.as_bytes());
            commitments.push(*hash.as_bytes());
        }

        Ok(commitments)
    }

    /// Get the commitment index for this session within the Merkle tree
    async fn get_session_commitment_index(&self) -> Result<usize, ProtocolError> {
        // In a real implementation, this would look up the session's commitment position
        // For now, use the last few bytes of session_id as index
        let session_bytes = self.session_id.as_bytes();
        let index = session_bytes[session_bytes.len() - 1] as usize % 3; // Modulo 3 for our simulated commitments
        Ok(index)
    }

    /// Get guardian share hashes from ledger state for Merkle tree construction
    async fn get_guardian_share_hashes(
        &self,
        guardian_id: GuardianId,
    ) -> Result<Vec<[u8; 32]>, ProtocolError> {
        // In a real implementation, this would query the ledger for guardian shares
        // For now, simulate with dummy guardian share hashes
        use blake3;

        let share_count = 2; // Simulate 2 guardian shares for this guardian
        let mut shares = Vec::new();

        for i in 0..share_count {
            // Generate deterministic share hash based on guardian and index
            let share_data = format!("{}:guardian_share:{}:{}", guardian_id.0, self.session_id, i);
            let hash = blake3::hash(share_data.as_bytes());
            shares.push(*hash.as_bytes());
        }

        Ok(shares)
    }
}

// ========== Merkle Proof Data Structures ==========

/// Serializable data structure for DKD commitment Merkle proofs
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MerkleProofData {
    /// Merkle root for verification
    pub merkle_root: [u8; 32],
    /// The actual Merkle proof
    pub proof: aura_crypto::MerkleProof,
    /// Session ID this proof is for
    pub session_id: Uuid,
}

/// Serializable data structure for guardian recovery Merkle proofs  
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GuardianMerkleProofData {
    /// Merkle root for verification
    pub merkle_root: [u8; 32],
    /// The actual Merkle proof
    pub proof: aura_crypto::MerkleProof,
    /// Guardian ID this proof is for
    pub guardian_id: GuardianId,
    /// Session ID this proof was generated in
    pub session_id: Uuid,
    /// Total number of shares included in the tree
    pub share_count: usize,
}
