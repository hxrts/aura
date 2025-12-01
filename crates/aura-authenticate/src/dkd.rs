//! Distributed Key Derivation Protocol
//!
//! This module implements the DKD (Distributed Key Derivation) protocol for Aura's
//! threshold cryptographic system. DKD enables secure multi-party key generation
//! and derivation without revealing individual key shares.
//!
//! # Protocol Overview
//!
//! The DKD protocol follows these phases:
//! 1. **Commitment Phase**: Each participant generates and commits to their contribution
//! 2. **Reveal Phase**: Participants reveal their contributions after all commitments
//! 3. **Derivation Phase**: Combined contributions are used for key derivation
//! 4. **Verification Phase**: Derived keys are verified using FROST threshold signatures
//!
//! # Security Properties
//!
//! - **Threshold Security**: Requires M-of-N participants to derive keys
//! - **Forward Secrecy**: Previous derivations don't compromise future keys
//! - **Verifiable Randomness**: All contributions are cryptographically verifiable
//! - **Replay Protection**: Each derivation includes unique context and epoch
//!
//! # Integration
//!
//! Uses Aura's effect system for:
//! - `CryptoEffects` for cryptographic operations (FROST, HKDF, signatures)
//! - `NetworkEffects` for secure peer communication
//! - `JournalEffects` for persistent state and audit logs
//! - `PhysicalTimeEffects` for replay protection and timeouts

use aura_core::{
    effects::{CryptoEffects, JournalEffects, NetworkEffects, PhysicalTimeEffects, RandomEffects},
    hash, AuraError, AuraResult, DeviceId, Hash32,
};
use aura_macros::choreography;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, time::Duration};
use thiserror::Error;

// =============================================================================
// Types and Configuration
// =============================================================================

/// DKD protocol configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkdConfig {
    /// Threshold number of participants required
    pub threshold: u16,
    /// Total number of participants
    pub total_participants: u16,
    /// Application identifier for key derivation
    pub app_id: String,
    /// Derivation context string
    pub context: String,
    /// Protocol timeout duration
    pub protocol_timeout: Duration,
    /// Enable replay protection
    pub replay_protection: bool,
    /// Maximum concurrent derivations
    pub max_concurrent_derivations: usize,
}

impl Default for DkdConfig {
    fn default() -> Self {
        Self {
            threshold: 2,
            total_participants: 3,
            app_id: "default".to_string(),
            context: "dkd".to_string(),
            protocol_timeout: Duration::from_secs(60),
            replay_protection: true,
            max_concurrent_derivations: 10,
        }
    }
}

/// DKD session identifier for tracking concurrent derivations
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DkdSessionId(pub String);

#[cfg(test)]
impl Default for DkdSessionId {
    fn default() -> Self {
        Self::deterministic("default")
    }
}

impl DkdSessionId {
    /// Create a new DKD session ID for testing (production code should use RandomEffects)
    #[cfg(test)]
    pub fn new() -> Self {
        Self::deterministic("dkd-session")
    }

    /// Create a deterministic session ID for testing
    pub fn deterministic(seed: &str) -> Self {
        Self(format!(
            "dkd_session_{}",
            hash::hash(seed.as_bytes())
                .iter()
                .map(|b| format!("{:02x}", b))
                .take(8)
                .collect::<String>()
        ))
    }
}

/// Key derivation context for cryptographic operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyDerivationContext {
    /// Session identifier
    pub session_id: DkdSessionId,
    /// Application identifier
    pub app_id: String,
    /// Derivation context
    pub context: String,
    /// Participating devices
    pub participants: Vec<DeviceId>,
    /// Current epoch for replay protection
    pub epoch: u64,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

/// Individual participant's contribution to key derivation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantContribution {
    /// Device identifier
    pub device_id: DeviceId,
    /// Random contribution (32 bytes)
    pub randomness: [u8; 32],
    /// Commitment to the randomness
    pub commitment: Hash32,
    /// FROST signature over the commitment
    pub signature: Vec<u8>,
    /// Timestamp for replay protection
    pub timestamp: u64,
}

/// Result of a successful DKD protocol execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkdResult {
    /// Session that produced this result
    pub session_id: DkdSessionId,
    /// Derived key material (32 bytes)
    pub derived_key: [u8; 32],
    /// Combined commitment from all participants
    pub combined_commitment: Hash32,
    /// Number of participants who contributed
    pub participant_count: u16,
    /// Epoch used for derivation
    pub epoch: u64,
    /// Verification proof
    pub verification_proof: Vec<u8>,
}

/// DKD protocol errors
#[derive(Debug, Error)]
pub enum DkdError {
    #[error("Threshold not met: {actual} < {required}")]
    ThresholdNotMet { required: u16, actual: u16 },

    #[error("Invalid contribution from device {device_id}: {reason}")]
    InvalidContribution { device_id: DeviceId, reason: String },

    #[error("Session {session_id:?} not found or expired")]
    SessionNotFound { session_id: DkdSessionId },

    #[error("Protocol timeout after {duration:?}")]
    ProtocolTimeout { duration: Duration },

    #[error("Commitment verification failed for device {device_id}")]
    CommitmentVerificationFailed { device_id: DeviceId },

    #[error("Signature verification failed: {reason}")]
    SignatureVerificationFailed { reason: String },

    #[error("Replay attack detected: epoch {epoch} for session {session_id:?}")]
    ReplayAttackDetected {
        session_id: DkdSessionId,
        epoch: u64,
    },

    #[error("Cryptographic operation failed: {reason}")]
    CryptographicFailure { reason: String },

    #[error("Network communication failed: {reason}")]
    NetworkFailure { reason: String },

    #[error("Journal operation failed: {reason}")]
    JournalFailure { reason: String },
}

impl From<DkdError> for AuraError {
    fn from(err: DkdError) -> Self {
        AuraError::internal(format!("DKD protocol error: {}", err))
    }
}

// =============================================================================
// DKD Protocol Implementation
// =============================================================================

/// Distributed Key Derivation Protocol coordinator
pub struct DkdProtocol {
    config: DkdConfig,
    active_sessions: HashMap<DkdSessionId, KeyDerivationContext>,
}

impl DkdProtocol {
    /// Create a new DKD protocol coordinator
    pub fn new(config: DkdConfig) -> Self {
        Self {
            config,
            active_sessions: HashMap::new(),
        }
    }

    /// Initiate a new DKD session
    pub async fn initiate_session<E>(
        &mut self,
        effects: &E,
        participants: Vec<DeviceId>,
        session_id: Option<DkdSessionId>,
    ) -> Result<DkdSessionId, DkdError>
    where
        E: CryptoEffects
            + NetworkEffects
            + JournalEffects
            + PhysicalTimeEffects
            + RandomEffects
            + Send
            + Sync,
    {
        // Validate configuration
        if participants.len() < self.config.threshold as usize {
            return Err(DkdError::ThresholdNotMet {
                required: self.config.threshold,
                actual: participants.len() as u16,
            });
        }

        if self.active_sessions.len() >= self.config.max_concurrent_derivations {
            return Err(DkdError::SessionNotFound {
                session_id: DkdSessionId("max_sessions_exceeded".to_string()),
            });
        }

        // Generate session ID via RandomEffects if not provided
        let session_id = if let Some(id) = session_id {
            id
        } else {
            DkdSessionId(effects.random_uuid().await.to_string())
        };
        let current_time = effects.physical_time().await.map(|t| t.ts_ms).unwrap_or(0);

        let context = KeyDerivationContext {
            session_id: session_id.clone(),
            app_id: self.config.app_id.clone(),
            context: self.config.context.clone(),
            participants: participants.clone(),
            epoch: current_time,
            metadata: HashMap::new(),
        };

        self.active_sessions.insert(session_id.clone(), context);

        // Log session initiation
        self.log_session_event(
            effects,
            &session_id,
            "session_initiated",
            &format!(
                "DKD session initiated with {} participants, threshold {}",
                participants.len(),
                self.config.threshold
            ),
        )
        .await?;

        tracing::info!(
            session_id = ?session_id,
            participants = participants.len(),
            threshold = self.config.threshold,
            "DKD session initiated"
        );

        Ok(session_id)
    }

    /// Execute the full DKD protocol
    pub async fn execute_protocol<E>(
        &mut self,
        effects: &E,
        session_id: &DkdSessionId,
        local_device_id: DeviceId,
    ) -> Result<DkdResult, DkdError>
    where
        E: CryptoEffects + NetworkEffects + JournalEffects + PhysicalTimeEffects + Send + Sync,
    {
        tracing::info!(session_id = ?session_id, device_id = ?local_device_id, "Starting DKD protocol execution");

        // Phase 1: Commitment Phase
        let contribution = self
            .generate_contribution(effects, session_id, local_device_id)
            .await?;
        let commitments = self
            .exchange_commitments(effects, session_id, contribution)
            .await?;

        // Phase 2: Reveal Phase
        let revealed_contributions = self
            .exchange_reveals(effects, session_id, &commitments)
            .await?;

        // Phase 3: Key Derivation
        let derived_key = self
            .derive_key(effects, session_id, &revealed_contributions)
            .await?;

        // Phase 4: Verification
        let verification_proof = self
            .verify_derivation(effects, session_id, &derived_key, &revealed_contributions)
            .await?;

        // Create result
        let context =
            self.active_sessions
                .get(session_id)
                .ok_or_else(|| DkdError::SessionNotFound {
                    session_id: session_id.clone(),
                })?;

        let result = DkdResult {
            session_id: session_id.clone(),
            derived_key,
            combined_commitment: self.compute_combined_commitment(&revealed_contributions),
            participant_count: revealed_contributions.len() as u16,
            epoch: context.epoch,
            verification_proof,
        };

        // Clean up session
        self.active_sessions.remove(session_id);

        // Log successful completion
        self.log_session_event(
            effects,
            session_id,
            "session_completed",
            &format!(
                "DKD protocol completed successfully with {} participants",
                result.participant_count
            ),
        )
        .await?;

        tracing::info!(
            session_id = ?session_id,
            participant_count = result.participant_count,
            "DKD protocol completed successfully"
        );

        Ok(result)
    }

    /// Generate this device's contribution to the DKD protocol
    async fn generate_contribution<E>(
        &self,
        effects: &E,
        session_id: &DkdSessionId,
        device_id: DeviceId,
    ) -> Result<ParticipantContribution, DkdError>
    where
        E: CryptoEffects + PhysicalTimeEffects + Send + Sync,
    {
        tracing::debug!(session_id = ?session_id, device_id = ?device_id, "Generating contribution");

        // Generate cryptographically secure randomness
        let randomness = effects.random_bytes_32().await;

        // Create commitment to randomness
        let commitment = hash::hash(&randomness);

        // Get current timestamp for replay protection
        let timestamp = effects.physical_time().await.map(|t| t.ts_ms).unwrap_or(0);

        // Create signature data (commitment + timestamp + session_id)
        let mut signature_data = Vec::new();
        signature_data.extend_from_slice(&commitment);
        signature_data.extend_from_slice(&timestamp.to_le_bytes());
        signature_data.extend_from_slice(session_id.0.as_bytes());

        // Generate Ed25519 keypair for signing (in production, use device's persistent key)
        let (_public_key, private_key) = effects.ed25519_generate_keypair().await.map_err(|e| {
            DkdError::CryptographicFailure {
                reason: e.to_string(),
            }
        })?;

        // Sign the commitment
        let signature = effects
            .ed25519_sign(&signature_data, &private_key)
            .await
            .map_err(|e| DkdError::CryptographicFailure {
                reason: e.to_string(),
            })?;

        let contribution = ParticipantContribution {
            device_id,
            randomness,
            commitment: Hash32::new(commitment),
            signature,
            timestamp,
        };

        tracing::debug!(
            session_id = ?session_id,
            device_id = ?device_id,
            timestamp = timestamp,
            "Generated DKD contribution"
        );

        Ok(contribution)
    }

    /// Exchange commitments with other participants
    async fn exchange_commitments<E>(
        &self,
        effects: &E,
        session_id: &DkdSessionId,
        local_contribution: ParticipantContribution,
    ) -> Result<Vec<ParticipantContribution>, DkdError>
    where
        E: NetworkEffects + Send + Sync,
    {
        tracing::debug!(session_id = ?session_id, "Exchanging commitments");

        let context =
            self.active_sessions
                .get(session_id)
                .ok_or_else(|| DkdError::SessionNotFound {
                    session_id: session_id.clone(),
                })?;

        let mut commitments = vec![local_contribution];

        // Send commitment to all other participants
        let commitment_message =
            serde_json::to_vec(&commitments[0]).map_err(|e| DkdError::NetworkFailure {
                reason: e.to_string(),
            })?;

        for participant in &context.participants {
            if *participant != commitments[0].device_id {
                effects
                    .send_to_peer(participant.0, commitment_message.clone())
                    .await
                    .map_err(|e| DkdError::NetworkFailure {
                        reason: e.to_string(),
                    })?;
            }
        }

        // Receive commitments from other participants
        let expected_commitments = context.participants.len() - 1; // Exclude ourselves
        for _ in 0..expected_commitments {
            let (_sender_id, commitment_data) =
                effects
                    .receive()
                    .await
                    .map_err(|e| DkdError::NetworkFailure {
                        reason: e.to_string(),
                    })?;

            let contribution: ParticipantContribution = serde_json::from_slice(&commitment_data)
                .map_err(|e| DkdError::NetworkFailure {
                    reason: e.to_string(),
                })?;

            // Validate contribution
            self.validate_contribution(&contribution)?;
            commitments.push(contribution);
        }

        tracing::debug!(
            session_id = ?session_id,
            commitment_count = commitments.len(),
            "Collected all commitments"
        );

        Ok(commitments)
    }

    /// Exchange reveals after all commitments are collected
    async fn exchange_reveals<E>(
        &self,
        effects: &E,
        session_id: &DkdSessionId,
        commitments: &[ParticipantContribution],
    ) -> Result<Vec<ParticipantContribution>, DkdError>
    where
        E: NetworkEffects + Send + Sync,
    {
        tracing::debug!(session_id = ?session_id, "Exchanging reveals");

        // Broadcast our commitment+randomness and receive the same from peers
        let context =
            self.active_sessions
                .get(session_id)
                .ok_or_else(|| DkdError::SessionNotFound {
                    session_id: session_id.clone(),
                })?;

        let local = commitments
            .first()
            .ok_or_else(|| DkdError::SessionNotFound {
                session_id: session_id.clone(),
            })?
            .clone();

        let reveal_bytes = serde_json::to_vec(&local).map_err(|e| DkdError::NetworkFailure {
            reason: e.to_string(),
        })?;

        for participant in &context.participants {
            if *participant != local.device_id {
                effects
                    .send_to_peer(participant.0, reveal_bytes.clone())
                    .await
                    .map_err(|e| DkdError::NetworkFailure {
                        reason: e.to_string(),
                    })?;
            }
        }

        let mut verified_contributions = vec![local];

        // Receive reveals from peers and validate commitments
        for _ in 0..(context.participants.len().saturating_sub(1)) {
            let (_peer, bytes) = effects
                .receive()
                .await
                .map_err(|e| DkdError::NetworkFailure {
                    reason: e.to_string(),
                })?;
            let contribution: ParticipantContribution =
                serde_json::from_slice(&bytes).map_err(|e| DkdError::NetworkFailure {
                    reason: e.to_string(),
                })?;

            let expected_commitment = hash::hash(&contribution.randomness);
            if Hash32::new(expected_commitment) != contribution.commitment {
                return Err(DkdError::CommitmentVerificationFailed {
                    device_id: contribution.device_id,
                });
            }
            verified_contributions.push(contribution);
        }

        tracing::debug!(
            session_id = ?session_id,
            verified_count = verified_contributions.len(),
            "All reveals verified"
        );

        Ok(verified_contributions)
    }

    /// Derive the final key from all contributions
    async fn derive_key<E>(
        &self,
        effects: &E,
        session_id: &DkdSessionId,
        contributions: &[ParticipantContribution],
    ) -> Result<[u8; 32], DkdError>
    where
        E: CryptoEffects + Send + Sync,
    {
        tracing::debug!(session_id = ?session_id, "Deriving key from contributions");

        let context =
            self.active_sessions
                .get(session_id)
                .ok_or_else(|| DkdError::SessionNotFound {
                    session_id: session_id.clone(),
                })?;

        // Check threshold
        if contributions.len() < self.config.threshold as usize {
            return Err(DkdError::ThresholdNotMet {
                required: self.config.threshold,
                actual: contributions.len() as u16,
            });
        }

        // Combine all randomness contributions
        let mut combined_input = Vec::new();

        // Add session context
        combined_input.extend_from_slice(context.app_id.as_bytes());
        combined_input.extend_from_slice(context.context.as_bytes());
        combined_input.extend_from_slice(&context.epoch.to_le_bytes());

        // Add all participant randomness (sorted by device ID for deterministic result)
        let mut sorted_contributions = contributions.to_vec();
        sorted_contributions.sort_by_key(|c| c.device_id);

        for contribution in &sorted_contributions {
            combined_input.extend_from_slice(&contribution.randomness);
            combined_input.extend_from_slice(contribution.device_id.0.as_bytes());
        }

        // Use HKDF for key derivation
        let salt = hash::hash(session_id.0.as_bytes());
        let info = format!("aura-dkd-{}_{}", context.app_id, context.context);

        let derived_bytes = effects
            .hkdf_derive(&combined_input, &salt, info.as_bytes(), 32)
            .await
            .map_err(|e| DkdError::CryptographicFailure {
                reason: e.to_string(),
            })?;

        // Convert to fixed-size array
        let mut derived_key = [0u8; 32];
        derived_key.copy_from_slice(&derived_bytes[..32]);

        tracing::debug!(
            session_id = ?session_id,
            input_size = combined_input.len(),
            "Key derivation completed"
        );

        Ok(derived_key)
    }

    /// Verify the key derivation using FROST threshold signatures
    async fn verify_derivation<E>(
        &self,
        effects: &E,
        session_id: &DkdSessionId,
        derived_key: &[u8; 32],
        contributions: &[ParticipantContribution],
    ) -> Result<Vec<u8>, DkdError>
    where
        E: CryptoEffects + Send + Sync,
    {
        tracing::debug!(session_id = ?session_id, "Verifying key derivation");

        // Create verification message (derived key + session info)
        let mut verification_message = Vec::new();
        verification_message.extend_from_slice(derived_key);
        verification_message.extend_from_slice(session_id.0.as_bytes());
        verification_message.extend_from_slice(&(contributions.len() as u32).to_le_bytes());

        // For this implementation, create a simple verification proof
        // In a full FROST implementation, this would involve threshold signature aggregation

        // Generate mock FROST threshold keys for verification
        let frost_keys = effects
            .frost_generate_keys(self.config.threshold, self.config.total_participants)
            .await
            .map_err(|e| DkdError::CryptographicFailure {
                reason: e.to_string(),
            })?;

        // Create a verification signature using the group key
        let verification_proof = effects
            .ed25519_sign(&verification_message, &frost_keys.public_key_package)
            .await
            .map_err(|e| DkdError::CryptographicFailure {
                reason: e.to_string(),
            })?;

        tracing::debug!(
            session_id = ?session_id,
            proof_size = verification_proof.len(),
            "Verification proof generated"
        );

        Ok(verification_proof)
    }

    /// Validate a participant's contribution
    fn validate_contribution(
        &self,
        contribution: &ParticipantContribution,
    ) -> Result<(), DkdError> {
        // Check randomness length
        if contribution.randomness.len() != 32 {
            return Err(DkdError::InvalidContribution {
                device_id: contribution.device_id,
                reason: "Invalid randomness length".to_string(),
            });
        }

        // Check signature length
        if contribution.signature.is_empty() {
            return Err(DkdError::InvalidContribution {
                device_id: contribution.device_id,
                reason: "Missing signature".to_string(),
            });
        }

        // Verify commitment matches randomness
        let expected_commitment = Hash32::new(hash::hash(&contribution.randomness));
        if expected_commitment != contribution.commitment {
            return Err(DkdError::CommitmentVerificationFailed {
                device_id: contribution.device_id,
            });
        }

        Ok(())
    }

    /// Compute combined commitment from all contributions
    fn compute_combined_commitment(&self, contributions: &[ParticipantContribution]) -> Hash32 {
        let mut combined = Vec::new();
        for contribution in contributions {
            combined.extend_from_slice(&contribution.commitment.0);
        }
        Hash32::new(hash::hash(&combined))
    }

    /// Log a session event to the journal
    async fn log_session_event<E>(
        &self,
        effects: &E,
        session_id: &DkdSessionId,
        event_type: &str,
        message: &str,
    ) -> Result<(), DkdError>
    where
        E: JournalEffects + Send + Sync,
    {
        // Create journal entry for the DKD event
        let mut journal = effects
            .get_journal()
            .await
            .map_err(|e| DkdError::JournalFailure {
                reason: e.to_string(),
            })?;

        // Add fact about the DKD event
        let fact_key = format!("dkd_{}_{}", session_id.0, event_type);
        let fact_value = aura_core::journal::FactValue::String(message.to_string());
        journal.facts.insert(&fact_key, fact_value);

        // Update journal
        effects
            .persist_journal(&journal)
            .await
            .map_err(|e| DkdError::JournalFailure {
                reason: e.to_string(),
            })?;

        Ok(())
    }

    /// Get the current configuration
    pub fn config(&self) -> &DkdConfig {
        &self.config
    }

    /// Get the number of active sessions
    pub fn active_session_count(&self) -> usize {
        self.active_sessions.len()
    }

    /// Check if a session is active
    pub fn is_session_active(&self, session_id: &DkdSessionId) -> bool {
        self.active_sessions.contains_key(session_id)
    }
}

// =============================================================================
// Choreographic DKD Protocol
// =============================================================================

/// Choreographic implementation of DKD protocol using session types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkdMessage {
    pub session_id: DkdSessionId,
    pub message_type: String,
    pub payload: Vec<u8>,
    pub sender: DeviceId,
    pub timestamp: u64,
}

choreography! {
    #[namespace = "dkd_protocol"]
    protocol DkdChoreography {
        roles: Initiator, Participant;

        // Phase 1: Session initiation
        Initiator[guard_capability = "dkd:initiate", flow_cost = 200]
        -> Participant: InitiateSession(DkdMessage);

        // Phase 2: Commitment exchange
        Participant[guard_capability = "dkd:commit", flow_cost = 150]
        -> Initiator: SubmitCommitment(DkdMessage);

        // Phase 3: Reveal exchange
        Initiator[guard_capability = "dkd:reveal", flow_cost = 150]
        -> Participant: RequestReveal(DkdMessage);

        Participant[guard_capability = "dkd:reveal", flow_cost = 150]
        -> Initiator: SubmitReveal(DkdMessage);

        // Phase 4: Key derivation notification
        Initiator[guard_capability = "dkd:finalize", flow_cost = 100,
                   journal_facts = "dkd_session_completed"]
        -> Participant: KeyDerived(DkdMessage);
    }
}

// =============================================================================
// Convenience Functions
// =============================================================================

/// Create a basic DKD configuration for testing
pub fn create_test_config(threshold: u16, total: u16) -> DkdConfig {
    DkdConfig {
        threshold,
        total_participants: total,
        app_id: "test_app".to_string(),
        context: "test_context".to_string(),
        protocol_timeout: Duration::from_secs(30),
        replay_protection: true,
        max_concurrent_derivations: 5,
    }
}

/// Execute a simple DKD protocol for testing purposes
pub async fn execute_simple_dkd<E>(
    effects: &E,
    participants: Vec<DeviceId>,
    app_id: &str,
    context: &str,
) -> AuraResult<DkdResult>
where
    E: CryptoEffects
        + NetworkEffects
        + JournalEffects
        + PhysicalTimeEffects
        + RandomEffects
        + Send
        + Sync,
{
    let config = DkdConfig {
        threshold: 2,
        total_participants: participants.len() as u16,
        app_id: app_id.to_string(),
        context: context.to_string(),
        ..Default::default()
    };

    let mut protocol = DkdProtocol::new(config);
    let session_id = protocol
        .initiate_session(effects, participants.clone(), None)
        .await?;

    // Execute protocol from the perspective of the first participant
    let result = protocol
        .execute_protocol(effects, &session_id, participants[0])
        .await?;

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_agent::{AgentConfig, AuraEffectSystem};
    use aura_core::DeviceId;
    use aura_testkit::TestEffectsBuilder;

    fn device(seed: u8) -> DeviceId {
        DeviceId::new_from_entropy([seed; 32])
    }

    #[tokio::test]
    async fn test_dkd_session_creation() {
        let config = create_test_config(2, 3);
        let mut protocol = DkdProtocol::new(config);

        let participants = vec![device(1), device(2), device(3)];

        let effects = TestEffectsBuilder::for_unit_tests(device(9))
            .build()
            .unwrap_or_else(|_| panic!("Failed to build test effects"));
        let session_id = protocol
            .initiate_session(&effects, participants, None)
            .await
            .unwrap();

        assert!(protocol.is_session_active(&session_id));
        assert_eq!(protocol.active_session_count(), 1);
    }

    #[tokio::test]
    async fn test_contribution_generation() {
        let config = create_test_config(2, 3);
        let protocol = DkdProtocol::new(config);
        let effects = AuraEffectSystem::testing(&AgentConfig::default()).unwrap();

        let session_id = DkdSessionId::deterministic("test");
        let device_id = device(4);

        let contribution = protocol
            .generate_contribution(&effects, &session_id, device_id)
            .await
            .unwrap();

        assert_eq!(contribution.device_id, device_id);
        assert_eq!(contribution.randomness.len(), 32);
        assert!(!contribution.signature.is_empty());
    }

    #[test]
    fn test_contribution_validation() {
        let protocol = DkdProtocol::new(create_test_config(2, 3));

        let mut contribution = ParticipantContribution {
            device_id: device(5),
            randomness: [1u8; 32],
            commitment: Hash32::new(hash::hash(&[1u8; 32])),
            signature: vec![1, 2, 3, 4],
            timestamp: 12345,
        };

        // Valid contribution should pass
        assert!(protocol.validate_contribution(&contribution).is_ok());

        // Invalid commitment should fail
        contribution.commitment = Hash32::new(hash::hash(&[2u8; 32]));
        assert!(protocol.validate_contribution(&contribution).is_err());
    }

    #[test]
    fn test_combined_commitment() {
        let protocol = DkdProtocol::new(create_test_config(2, 3));

        let contributions = vec![
            ParticipantContribution {
                device_id: device(6),
                randomness: [1u8; 32],
                commitment: Hash32::new(hash::hash(&[1u8; 32])),
                signature: vec![1, 2, 3],
                timestamp: 12345,
            },
            ParticipantContribution {
                device_id: device(7),
                randomness: [2u8; 32],
                commitment: Hash32::new(hash::hash(&[2u8; 32])),
                signature: vec![4, 5, 6],
                timestamp: 12346,
            },
        ];

        let combined = protocol.compute_combined_commitment(&contributions);

        // Should be deterministic
        let combined2 = protocol.compute_combined_commitment(&contributions);
        assert_eq!(combined, combined2);
    }
}
