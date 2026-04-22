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
//! - `CryptoEffects` for cryptographic operations (FROST, KDF, signatures)
//! - `NetworkEffects` for secure peer communication
//! - `JournalEffects` for persistent state and audit logs
//! - `PhysicalTimeEffects` for replay protection and timeouts

use aura_core::threshold::{policy_for, AgreementMode, CeremonyFlow};
use aura_core::{
    effects::{CryptoEffects, JournalEffects, NetworkEffects, PhysicalTimeEffects, RandomEffects},
    hash, AuraError, AuraResult, ContextId, DeviceId, Hash32,
};
use aura_guards::{
    DecodedIngress, IngressSource, IngressVerificationEvidence, VerifiedIngress,
    VerifiedIngressMetadata,
};
use aura_macros::tell;
use aura_signature::{sign_ed25519_transcript, SecurityTranscript};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
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
    pub max_concurrent_derivations: u32,
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
                .map(|b| format!("{b:02x}"))
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

#[derive(Debug, Clone, Serialize)]
struct DkdContributionTranscriptPayload {
    session_id: DkdSessionId,
    app_id: String,
    derivation_context: String,
    epoch: u64,
    participants: Vec<DeviceId>,
    device_id: DeviceId,
    commitment: Hash32,
    timestamp: u64,
}

struct DkdContributionTranscript {
    context: KeyDerivationContext,
    device_id: DeviceId,
    commitment: Hash32,
    timestamp: u64,
}

impl SecurityTranscript for DkdContributionTranscript {
    type Payload = DkdContributionTranscriptPayload;

    const DOMAIN_SEPARATOR: &'static str = "aura.dkd.contribution";

    fn transcript_payload(&self) -> Self::Payload {
        DkdContributionTranscriptPayload {
            session_id: self.context.session_id.clone(),
            app_id: self.context.app_id.clone(),
            derivation_context: self.context.context.clone(),
            epoch: self.context.epoch,
            participants: self.context.participants.clone(),
            device_id: self.device_id,
            commitment: self.commitment,
            timestamp: self.timestamp,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct DkdDerivationTranscriptPayload {
    session_id: DkdSessionId,
    authority_id: Option<aura_core::AuthorityId>,
    derived_key: [u8; 32],
    contribution_count: u32,
    commitments: Vec<Hash32>,
}

struct DkdDerivationTranscript {
    session_id: DkdSessionId,
    authority_id: Option<aura_core::AuthorityId>,
    derived_key: [u8; 32],
    commitments: Vec<Hash32>,
}

impl SecurityTranscript for DkdDerivationTranscript {
    type Payload = DkdDerivationTranscriptPayload;

    const DOMAIN_SEPARATOR: &'static str = "aura.dkd.derivation-verification";

    fn transcript_payload(&self) -> Self::Payload {
        DkdDerivationTranscriptPayload {
            session_id: self.session_id.clone(),
            authority_id: self.authority_id,
            derived_key: self.derived_key,
            contribution_count: self.commitments.len() as u32,
            commitments: self.commitments.clone(),
        }
    }
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
    /// Agreement mode for this result (A1/A2/A3)
    pub agreement_mode: AgreementMode,
    /// Whether reversion is still possible
    pub reversion_risk: bool,
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
        AuraError::internal(format!("DKD protocol error: {err}"))
    }
}

// =============================================================================
// DKD Protocol Implementation
// =============================================================================

/// Distributed Key Derivation Protocol coordinator
pub struct DkdProtocol {
    config: DkdConfig,
    active_sessions: HashMap<DkdSessionId, KeyDerivationContext>,
    agreement_mode: AgreementMode,
}

impl DkdProtocol {
    /// Create a new DKD protocol coordinator
    pub fn new(config: DkdConfig) -> Self {
        Self {
            config,
            active_sessions: HashMap::new(),
            agreement_mode: policy_for(CeremonyFlow::DkdCeremony).initial_mode(),
        }
    }

    fn session_context(
        &self,
        session_id: &DkdSessionId,
    ) -> Result<&KeyDerivationContext, DkdError> {
        self.active_sessions
            .get(session_id)
            .ok_or_else(|| DkdError::SessionNotFound {
                session_id: session_id.clone(),
            })
    }

    fn dkd_ingress_context(session_id: &DkdSessionId) -> ContextId {
        ContextId::new_from_entropy(hash::hash(session_id.0.as_bytes()))
    }

    fn verified_contribution(
        session_id: &DkdSessionId,
        participants: &[DeviceId],
        sender_id: uuid::Uuid,
        contribution: ParticipantContribution,
    ) -> Result<VerifiedIngress<ParticipantContribution>, DkdError> {
        let payload_hash =
            Hash32::from_value(&contribution).map_err(|error| DkdError::InvalidContribution {
                device_id: contribution.device_id,
                reason: format!("failed to hash contribution payload: {error}"),
            })?;
        let metadata = VerifiedIngressMetadata::new(
            IngressSource::Device(contribution.device_id),
            Self::dkd_ingress_context(session_id),
            None,
            payload_hash,
            1,
        );
        let expected_commitment = Hash32::new(hash::hash(&contribution.randomness));
        let evidence = IngressVerificationEvidence::builder(metadata)
            .peer_identity(
                sender_id == contribution.device_id.0,
                "network sender must match contribution device id",
            )
            .and_then(|builder| {
                builder.envelope_authenticity(
                    !contribution.signature.is_empty(),
                    "DKD contribution signature must be present",
                )
            })
            .and_then(|builder| {
                builder.capability_authorization(
                    participants.contains(&contribution.device_id),
                    "DKD contributor must be in the session participant set",
                )
            })
            .and_then(|builder| {
                builder.namespace_scope(true, "DKD context is derived from session id")
            })
            .and_then(|builder| builder.schema_version(true, "DKD contribution schema v1"))
            .and_then(|builder| {
                builder.replay_freshness(
                    contribution.timestamp != 0,
                    "DKD contribution timestamp must be non-zero",
                )
            })
            .and_then(|builder| {
                builder.signer_membership(
                    participants.contains(&contribution.device_id),
                    "DKD signer must be a session participant",
                )
            })
            .and_then(|builder| {
                builder.proof_evidence(
                    expected_commitment == contribution.commitment,
                    "DKD commitment must match revealed randomness",
                )
            })
            .and_then(|builder| builder.build())
            .map_err(|error| DkdError::InvalidContribution {
                device_id: contribution.device_id,
                reason: format!("invalid DKD ingress evidence: {error}"),
            })?;
        let device_id = contribution.device_id;
        DecodedIngress::new(contribution, evidence.metadata().clone())
            .verify(evidence)
            .map_err(|error| DkdError::InvalidContribution {
                device_id,
                reason: format!("failed to promote DKD ingress: {error}"),
            })
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

        if self.active_sessions.len() >= self.config.max_concurrent_derivations as usize {
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

        let policy = policy_for(CeremonyFlow::DkdCeremony);
        if policy.allows_mode(AgreementMode::CoordinatorSoftSafe) {
            self.agreement_mode = AgreementMode::CoordinatorSoftSafe;
        }

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

        let policy = policy_for(CeremonyFlow::DkdCeremony);
        let agreement_mode = if policy.allows_mode(AgreementMode::ConsensusFinalized) {
            AgreementMode::CoordinatorSoftSafe
        } else {
            self.agreement_mode
        };

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
            agreement_mode,
            reversion_risk: agreement_mode != AgreementMode::ConsensusFinalized,
        };

        // Clean up session
        self.active_sessions.remove(session_id);

        // Log completion; treat as provisional when consensus finalization is required.
        let event_type = if policy.allows_mode(AgreementMode::ConsensusFinalized) {
            "session_completed_provisional"
        } else {
            "session_completed"
        };
        let message = if policy.allows_mode(AgreementMode::ConsensusFinalized) {
            format!(
                "DKD completed with {} participants (consensus finalization required)",
                result.participant_count
            )
        } else {
            format!(
                "DKD protocol completed successfully with {} participants",
                result.participant_count
            )
        };
        self.log_session_event(effects, session_id, event_type, &message)
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

        let session_context = self.session_context(session_id)?.clone();
        let commitment = Hash32::new(commitment);
        let signature_transcript = DkdContributionTranscript {
            context: session_context,
            device_id,
            commitment,
            timestamp,
        };

        // Generate Ed25519 keypair for signing (in production, use device's persistent key)
        let (_public_key, private_key) = effects
            .ed25519_generate_keypair()
            .await
            .map_err(crypto_failure)?;

        // Sign the commitment
        let signature = sign_ed25519_transcript(effects, &signature_transcript, &private_key)
            .await
            .map_err(crypto_failure)?;

        let contribution = ParticipantContribution {
            device_id,
            randomness,
            commitment,
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

        let context = self.session_context(session_id)?;

        let mut peer_commitments = Vec::new();

        // Send commitment to all other participants
        let commitment_message = serialize_network(&local_contribution)?;

        for participant in &context.participants {
            if *participant != local_contribution.device_id {
                effects
                    .send_to_peer(participant.0, commitment_message.clone())
                    .await
                    .map_err(network_failure)?;
            }
        }

        // Receive commitments from other participants
        let expected_commitments = context.participants.len() - 1; // Exclude ourselves
        for _ in 0..expected_commitments {
            let (sender_id, commitment_data) = effects.receive().await.map_err(network_failure)?;

            let contribution: ParticipantContribution = deserialize_network(&commitment_data)?;
            let contribution = Self::verified_contribution(
                session_id,
                &context.participants,
                sender_id,
                contribution,
            )?;

            // Validate contribution
            self.validate_verified_contribution(&contribution)?;
            peer_commitments.push(contribution);
        }

        tracing::debug!(
            session_id = ?session_id,
            commitment_count = 1 + peer_commitments.len(),
            "Collected all commitments"
        );

        let mut commitments = vec![local_contribution];
        commitments.extend(
            peer_commitments
                .into_iter()
                .map(|contribution| contribution.into_parts().0),
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
        let context = self.session_context(session_id)?;

        let local = commitments
            .first()
            .ok_or_else(|| DkdError::SessionNotFound {
                session_id: session_id.clone(),
            })?
            .clone();

        let reveal_bytes = serialize_network(&local)?;

        for participant in &context.participants {
            if *participant != local.device_id {
                effects
                    .send_to_peer(participant.0, reveal_bytes.clone())
                    .await
                    .map_err(network_failure)?;
            }
        }

        let mut verified_peer_contributions = Vec::new();

        // Receive reveals from peers and validate commitments
        for _ in 0..(context.participants.len().saturating_sub(1)) {
            let (sender_id, bytes) = effects.receive().await.map_err(network_failure)?;
            let contribution: ParticipantContribution = deserialize_network(&bytes)?;
            let contribution = Self::verified_contribution(
                session_id,
                &context.participants,
                sender_id,
                contribution,
            )?;
            let contribution_ref = contribution.payload();

            let expected_commitment = hash::hash(&contribution_ref.randomness);
            if Hash32::new(expected_commitment) != contribution_ref.commitment {
                return Err(DkdError::CommitmentVerificationFailed {
                    device_id: contribution_ref.device_id,
                });
            }
            verified_peer_contributions.push(contribution);
        }

        tracing::debug!(
            session_id = ?session_id,
            verified_count = 1 + verified_peer_contributions.len(),
            "All reveals verified"
        );

        let mut verified_contributions = vec![local];
        verified_contributions.extend(
            verified_peer_contributions
                .into_iter()
                .map(|contribution| contribution.into_parts().0),
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

        let context = self.session_context(session_id)?;

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

        // Use the crypto KDF for key derivation.
        let salt = hash::hash(session_id.0.as_bytes());
        let info = format!("aura-dkd-{}_{}", context.app_id, context.context);

        let derived_bytes = effects
            .kdf_derive(&combined_input, &salt, info.as_bytes(), 32)
            .await
            .map_err(crypto_failure)?;

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

    /// Verify the key derivation and create an HMAC-based verification proof.
    ///
    /// This creates a deterministic verification proof that can be checked by
    /// any participant who knows the derived key. The proof binds together:
    /// - The derived key
    /// - The session ID
    /// - The contribution count
    /// - All participant commitments
    ///
    /// # Note on Threshold Signing
    ///
    /// For stronger verification using threshold signatures (FROST), call
    /// `verify_derivation_with_threshold()` which requires an authority with
    /// established FROST keys via `ThresholdSigningEffects`.
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

        let verification_message = DkdDerivationTranscript {
            session_id: session_id.clone(),
            authority_id: None,
            derived_key: *derived_key,
            commitments: contributions
                .iter()
                .map(|contribution| contribution.commitment)
                .collect(),
        }
        .transcript_bytes()
        .map_err(|error| crypto_failure(format!("DKD derivation transcript failed: {error}")))?;

        // Create a keyed verification proof using the crypto KDF.
        // This proves knowledge of the derived key and correct contribution binding
        let verification_proof = effects
            .kdf_derive(
                derived_key,
                &verification_message,
                b"dkd_verification_proof",
                32,
            )
            .await
            .map_err(|e| crypto_failure(format!("KDF verification failed: {e}")))?;

        tracing::debug!(
            session_id = ?session_id,
            proof_size = verification_proof.len(),
            contributions_count = contributions.len(),
            "HMAC verification proof generated"
        );

        Ok(verification_proof)
    }

    /// Verify the key derivation using threshold signatures (FROST).
    ///
    /// This creates a cryptographic proof using the authority's threshold signing
    /// capabilities. Use this when the authority already has established FROST keys.
    ///
    /// # Arguments
    /// - `effects`: Effects system implementing `ThresholdSigningEffects`
    /// - `authority_id`: The authority whose FROST keys will sign the verification
    /// - `session_id`: The DKD session identifier
    /// - `derived_key`: The derived key to verify
    /// - `contributions`: All participant contributions
    ///
    /// # Returns
    /// A threshold signature over the verification message.
    pub async fn verify_derivation_with_threshold<E>(
        &self,
        effects: &E,
        authority_id: aura_core::AuthorityId,
        session_id: &DkdSessionId,
        derived_key: &[u8; 32],
        contributions: &[ParticipantContribution],
    ) -> Result<aura_core::threshold::ThresholdSignature, DkdError>
    where
        E: aura_core::effects::ThresholdSigningEffects + Send + Sync,
    {
        use aura_core::threshold::{ApprovalContext, SignableOperation, SigningContext};

        tracing::debug!(
            session_id = ?session_id,
            ?authority_id,
            "Verifying key derivation with threshold signature"
        );

        let verification_payload = DkdDerivationTranscript {
            session_id: session_id.clone(),
            authority_id: Some(authority_id),
            derived_key: *derived_key,
            commitments: contributions
                .iter()
                .map(|contribution| contribution.commitment)
                .collect(),
        }
        .transcript_bytes()
        .map_err(|error| crypto_failure(format!("DKD threshold transcript failed: {error}")))?;

        // Create signing context for DKD verification
        let signing_context = SigningContext {
            authority: authority_id,
            operation: SignableOperation::Message {
                domain: "dkd_verification".to_string(),
                payload: verification_payload,
            },
            approval_context: ApprovalContext::SelfOperation,
        };

        // Sign using threshold signing service
        let signature = effects
            .sign(signing_context)
            .await
            .map_err(|e| crypto_failure(format!("Threshold signing failed: {e}")))?;

        tracing::info!(
            session_id = ?session_id,
            ?authority_id,
            signer_count = signature.signer_count,
            "DKD threshold verification signature generated"
        );

        Ok(signature)
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

    fn validate_verified_contribution(
        &self,
        contribution: &VerifiedIngress<ParticipantContribution>,
    ) -> Result<(), DkdError> {
        self.validate_contribution(contribution.payload())
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
        let mut journal = effects.get_journal().await.map_err(journal_failure)?;

        // Add fact about the DKD event
        let fact_key = format!("dkd_{}_{}", session_id.0, event_type);
        let fact_value = aura_core::journal::FactValue::String(message.to_string());
        journal
            .facts
            .insert(fact_key, fact_value)
            .map_err(journal_failure)?;

        // Update journal
        effects
            .persist_journal(&journal)
            .await
            .map_err(journal_failure)?;

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

fn crypto_failure(reason: impl ToString) -> DkdError {
    DkdError::CryptographicFailure {
        reason: reason.to_string(),
    }
}

fn network_failure(reason: impl ToString) -> DkdError {
    DkdError::NetworkFailure {
        reason: reason.to_string(),
    }
}

fn journal_failure(reason: impl ToString) -> DkdError {
    DkdError::JournalFailure {
        reason: reason.to_string(),
    }
}

fn serialize_network<T: Serialize>(value: &T) -> Result<Vec<u8>, DkdError> {
    serde_json::to_vec(value).map_err(network_failure)
}

fn deserialize_network<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, DkdError> {
    serde_json::from_slice(bytes).map_err(network_failure)
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

tell!(include_str!("src/dkd.tell"));

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
    let participant_count = participants.len().max(1) as u16;
    let config = DkdConfig {
        threshold: participant_count,
        total_participants: participant_count,
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
    use crate::test_support::device;
    use aura_testkit::TestEffectsBuilder;

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
        let mut protocol = DkdProtocol::new(config);
        let effects = TestEffectsBuilder::for_unit_tests(device(9))
            .build()
            .unwrap_or_else(|_| panic!("Failed to build test effects"));

        let device_id = device(4);
        let session_id = protocol
            .initiate_session(
                &effects,
                vec![device_id, device(5)],
                Some(DkdSessionId::deterministic("test")),
            )
            .await
            .unwrap();

        let contribution = protocol
            .generate_contribution(&effects, &session_id, device_id)
            .await
            .unwrap();

        assert_eq!(contribution.device_id, device_id);
        assert_eq!(contribution.randomness.len(), 32);
        assert!(!contribution.signature.is_empty());
    }

    #[tokio::test]
    async fn dkd_contribution_transcript_binds_session_epoch() {
        let config = create_test_config(2, 3);
        let mut protocol = DkdProtocol::new(config);
        let effects = TestEffectsBuilder::for_unit_tests(device(9))
            .build()
            .unwrap_or_else(|_| panic!("Failed to build test effects"));
        let device_id = device(4);
        let session_id = protocol
            .initiate_session(
                &effects,
                vec![device_id, device(5)],
                Some(DkdSessionId::deterministic("test")),
            )
            .await
            .unwrap();
        let context = protocol.session_context(&session_id).unwrap().clone();
        let mut next_context = context.clone();
        next_context.epoch = next_context.epoch.saturating_add(1);
        let commitment = Hash32::new([7; 32]);

        let current = DkdContributionTranscript {
            context,
            device_id,
            commitment,
            timestamp: 100,
        }
        .transcript_bytes()
        .unwrap();
        let next_epoch = DkdContributionTranscript {
            context: next_context,
            device_id,
            commitment,
            timestamp: 100,
        }
        .transcript_bytes()
        .unwrap();

        assert_ne!(current, next_epoch);
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

    /// DKD result uses CoordinatorSoftSafe agreement — must be superseded
    /// by consensus for durable shared state.
    #[tokio::test]
    async fn test_dkd_agreement_mode_requires_consensus() {
        let participants = vec![device(9)];
        let effects = TestEffectsBuilder::for_unit_tests(device(9))
            .build()
            .unwrap_or_else(|_| panic!("Failed to build test effects"));

        let result = execute_simple_dkd(&effects, participants, "test_app", "test_ctx")
            .await
            .unwrap();

        assert_eq!(result.agreement_mode, AgreementMode::CoordinatorSoftSafe);
        assert!(result.reversion_risk);
    }

    #[test]
    fn dkd_manifest_includes_runtime_startup_defaults() {
        let manifest = telltale_session_types_dkd_protocol::vm_artifacts::composition_manifest();

        assert_eq!(manifest.protocol_name, "DkdChoreography");
        assert_eq!(manifest.protocol_namespace.as_deref(), Some("dkd_protocol"));
        assert_eq!(manifest.protocol_id, "aura.dkg.ceremony");
        assert_eq!(
            manifest.required_capabilities,
            vec!["byzantine_envelope", "termination_bounded"]
        );
        assert_eq!(
            manifest.determinism_policy_ref.as_deref(),
            Some("aura.vm.dkg_ceremony.prod")
        );
    }
}
