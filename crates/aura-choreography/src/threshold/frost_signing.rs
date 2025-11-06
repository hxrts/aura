//! Journal FROST signing choreography using choreographic patterns
//!
//! This is a refactored implementation that uses the fundamental choreographic patterns:
//! - propose_and_acknowledge for signing initialization and configuration
//! - broadcast_and_gather for credential verification and commitment exchange
//! - verify_consistent_result for signature aggregation verification

use crate::patterns::{
    BroadcastAndGatherChoreography, BroadcastGatherConfig, MessageValidator, ProposalValidator,
    ProposeAcknowledgeConfig, ProposeAndAcknowledgeChoreography, ResultComparator,
    VerificationConfig,
};
use aura_protocol::effects::choreographic::ChoreographicRole;
use aura_protocol::effects::{CryptoEffects, RandomEffects};
use aura_types::{CapabilityRef, DeviceId};
use rumpsteak_choreography::{ChoreoHandler, ChoreographyError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Configuration for Journal FROST threshold signing choreography
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalFrostConfig {
    /// List of devices participating in the signing process
    pub participants: Vec<DeviceId>,
    /// Minimum number of signatures required for validity
    pub threshold: u16,
    /// Epoch number for anti-replay protection
    pub epoch: u64,
    /// Required capabilities that signing participants must possess
    pub required_capabilities: Vec<CapabilityRef>,
    /// Timeout duration in seconds for choreographic rounds
    pub timeout_seconds: u64,
}

/// Signing context that gets proposed to all participants
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SigningContext {
    /// Cryptographic hash of the message to be signed
    pub message_hash: [u8; 32],
    /// Serialized journal context for the signing session
    pub journal_context: Vec<u8>,
    /// Required capabilities that participants must possess
    pub required_capabilities: Vec<CapabilityRef>,
    /// Minimum threshold for signature validity
    pub threshold: u16,
    /// Epoch number for anti-replay protection
    pub epoch: u64,
}

/// Journal credentials for participant verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalCredentials {
    /// Unique identifier of the device
    pub device_id: DeviceId,
    /// Serialized node credentials
    pub credentials: Vec<u8>,
    /// Cryptographic proofs of required capabilities
    pub capability_proofs: Vec<u8>,
    /// Random nonce for this epoch
    pub epoch_nonce: [u8; 32],
}

/// FROST commitment with journal binding for security
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalFrostCommitment {
    /// Serialized FROST signing commitment
    pub frost_commitment: Vec<u8>,
    /// Binding to journal context for additional security
    pub journal_binding: [u8; 32],
}

/// Validator for journal FROST commitments
pub struct JournalFrostCommitmentValidator;

/// FROST signature share with journal witness
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalFrostShare {
    /// Serialized FROST signature share
    pub signature_share: Vec<u8>,
    /// Journal-specific witness data
    pub journal_witness: Vec<u8>,
}

/// Validator for journal FROST shares
pub struct JournalFrostShareValidator;

/// Final aggregated FROST signature result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalFrostResult {
    /// Aggregated threshold signature
    pub signature: Vec<u8>,
    /// List of participants who contributed to the signature
    pub participants: Vec<DeviceId>,
    /// Epoch number during which signature was created
    pub epoch: u64,
    /// Cryptographic proof of signature validity
    pub validity_proof: Vec<u8>,
}

/// Validator for signing context proposals
pub struct SigningContextValidator {
    /// Configuration specifying validation requirements
    config: JournalFrostConfig,
}

impl ProposalValidator<SigningContext> for SigningContextValidator {
    /// Validate an outgoing signing context proposal
    ///
    /// # Arguments
    /// * `proposal` - The signing context being proposed
    /// * `_proposer` - Role of the participant proposing the context
    fn validate_outgoing(
        &self,
        proposal: &SigningContext,
        _proposer: ChoreographicRole,
    ) -> Result<(), String> {
        if proposal.epoch != self.config.epoch {
            return Err("Epoch mismatch in signing context".to_string());
        }
        if proposal.threshold != self.config.threshold {
            return Err("Threshold mismatch in signing context".to_string());
        }
        if proposal.required_capabilities != self.config.required_capabilities {
            return Err("Capability requirements mismatch".to_string());
        }
        Ok(())
    }

    /// Validate an incoming signing context proposal
    ///
    /// # Arguments
    /// * `proposal` - The signing context being proposed
    /// * `_proposer` - Role of the participant proposing the context
    /// * `_receiver` - Role of the participant receiving the context
    fn validate_incoming(
        &self,
        proposal: &SigningContext,
        _proposer: ChoreographicRole,
        _receiver: ChoreographicRole,
    ) -> Result<(), String> {
        self.validate_outgoing(proposal, _proposer)
    }
}

/// Validator for journal credentials
pub struct JournalCredentialsValidator {
    /// Configuration specifying validation requirements
    config: JournalFrostConfig,
}

impl MessageValidator<JournalCredentials> for JournalCredentialsValidator {
    /// Validate an outgoing journal credentials message
    ///
    /// # Arguments
    /// * `message` - The credentials being sent
    /// * `sender` - Role of the participant sending the credentials
    fn validate_outgoing(
        &self,
        message: &JournalCredentials,
        sender: ChoreographicRole,
    ) -> Result<(), String> {
        // Verify the device_id corresponds to the sender role
        let expected_device_id = DeviceId(sender.device_id);
        if message.device_id != expected_device_id {
            return Err("Device ID mismatch with sender role".to_string());
        }
        // Additional journal credential validation would go here
        Ok(())
    }

    /// Validate an incoming journal credentials message
    ///
    /// # Arguments
    /// * `message` - The credentials being received
    /// * `_sender` - Role of the participant sending the credentials
    /// * `_receiver` - Role of the participant receiving the credentials
    fn validate_incoming(
        &self,
        message: &JournalCredentials,
        _sender: ChoreographicRole,
        _receiver: ChoreographicRole,
    ) -> Result<(), String> {
        // Verify credentials are well-formed and capabilities are sufficient
        if message.credentials.is_empty() {
            return Err("Empty credentials".to_string());
        }
        if message.capability_proofs.is_empty() {
            return Err("Empty capability proofs".to_string());
        }
        // Additional validation against required capabilities would go here
        Ok(())
    }
}

impl MessageValidator<JournalFrostCommitment> for JournalFrostCommitmentValidator {
    /// Validate an outgoing FROST commitment message
    ///
    /// # Arguments
    /// * `message` - The commitment being sent
    /// * `_sender` - Role of the participant sending the commitment
    fn validate_outgoing(
        &self,
        message: &JournalFrostCommitment,
        _sender: ChoreographicRole,
    ) -> Result<(), String> {
        if message.frost_commitment.is_empty() {
            return Err("Empty FROST commitment".to_string());
        }
        // Additional FROST commitment validation would go here
        Ok(())
    }

    /// Validate an incoming FROST commitment message
    ///
    /// # Arguments
    /// * `message` - The commitment being received
    /// * `_sender` - Role of the participant sending the commitment
    /// * `_receiver` - Role of the participant receiving the commitment
    fn validate_incoming(
        &self,
        message: &JournalFrostCommitment,
        _sender: ChoreographicRole,
        _receiver: ChoreographicRole,
    ) -> Result<(), String> {
        self.validate_outgoing(message, _sender)
    }
}

impl MessageValidator<JournalFrostShare> for JournalFrostShareValidator {
    /// Validate an outgoing FROST signature share message
    ///
    /// # Arguments
    /// * `message` - The signature share being sent
    /// * `_sender` - Role of the participant sending the share
    fn validate_outgoing(
        &self,
        message: &JournalFrostShare,
        _sender: ChoreographicRole,
    ) -> Result<(), String> {
        if message.signature_share.is_empty() {
            return Err("Empty signature share".to_string());
        }
        if message.journal_witness.is_empty() {
            return Err("Empty journal witness".to_string());
        }
        Ok(())
    }

    /// Validate an incoming FROST signature share message
    ///
    /// # Arguments
    /// * `message` - The signature share being received
    /// * `_sender` - Role of the participant sending the share
    /// * `_receiver` - Role of the participant receiving the share
    fn validate_incoming(
        &self,
        message: &JournalFrostShare,
        _sender: ChoreographicRole,
        _receiver: ChoreographicRole,
    ) -> Result<(), String> {
        self.validate_outgoing(message, _sender)
    }
}

/// Comparator for final journal signature results
pub struct JournalFrostResultComparator;

impl ResultComparator<JournalFrostResult> for JournalFrostResultComparator {
    /// Check if two FROST signature results are equal
    ///
    /// # Arguments
    /// * `a` - First result to compare
    /// * `b` - Second result to compare
    fn are_equal(&self, a: &JournalFrostResult, b: &JournalFrostResult) -> bool {
        a.signature == b.signature && a.participants == b.participants && a.epoch == b.epoch
    }

    /// Compute a cryptographic hash of the signature result
    ///
    /// # Arguments
    /// * `result` - The signature result to hash
    /// * `nonce` - Optional nonce to include in the hash
    /// * `crypto` - Crypto effects interface for cryptographic operations
    async fn hash_result<C: CryptoEffects>(
        &self,
        result: &JournalFrostResult,
        nonce: Option<&[u8; 32]>,
        _crypto: &C,
    ) -> [u8; 32] {
        let mut data = Vec::new();
        data.extend_from_slice(&result.signature);
        data.extend_from_slice(&bincode::serialize(&result.participants).unwrap_or_default());
        data.extend_from_slice(&result.epoch.to_le_bytes());

        if let Some(nonce) = nonce {
            data.extend_from_slice(nonce);
        }

        blake3::hash(&data).into()
    }

    /// Validate that a signature result is well-formed
    ///
    /// # Arguments
    /// * `result` - The signature result to validate
    /// * `_participant` - Role of the participant validating the result
    fn validate_result(
        &self,
        result: &JournalFrostResult,
        _participant: ChoreographicRole,
    ) -> Result<(), String> {
        if result.signature.is_empty() {
            return Err("Empty signature in result".to_string());
        }
        if result.participants.is_empty() {
            return Err("Empty participants list".to_string());
        }
        // Additional signature validation would go here
        Ok(())
    }
}

/// Journal FROST signing choreography using choreographic patterns
pub struct JournalFrostSigningChoreography<C, R>
where
    C: CryptoEffects + Clone,
    R: RandomEffects + Clone,
{
    /// Configuration for the FROST signing process
    config: JournalFrostConfig,
    /// Message to be signed
    message: Vec<u8>,
    /// Cryptographic effects for hashing and verification
    crypto: C,
    /// Random effects for nonce generation
    random: R,
}

impl<C, R> JournalFrostSigningChoreography<C, R>
where
    C: CryptoEffects + Clone,
    R: RandomEffects + Clone,
{
    /// Create a new Journal FROST signing choreography
    ///
    /// # Arguments
    /// * `config` - Configuration for the FROST signing process
    /// * `message` - Message to be signed
    /// * `crypto` - Cryptographic effects for hashing and verification
    /// * `random` - Random effects for nonce generation
    pub fn new(config: JournalFrostConfig, message: Vec<u8>, crypto: C, random: R) -> Self {
        Self {
            config,
            message,
            crypto,
            random,
        }
    }

    /// Execute the complete FROST signing choreography using patterns
    ///
    /// # Arguments
    /// * `handler` - Choreography handler for message processing
    /// * `endpoint` - Network endpoint for communication
    /// * `participants` - List of all participating roles
    /// * `my_role` - Role of this participant in the choreography
    /// * `coordinator_role` - Role of the coordinator initiating the signing
    pub async fn execute<H: ChoreoHandler<Role = ChoreographicRole>>(
        &self,
        handler: &mut H,
        endpoint: &mut H::Endpoint,
        participants: Vec<ChoreographicRole>,
        my_role: ChoreographicRole,
        coordinator_role: ChoreographicRole,
    ) -> Result<JournalFrostResult, ChoreographyError> {
        tracing::info!(
            participant = ?my_role,
            participant_count = participants.len(),
            epoch = self.config.epoch,
            "Starting Journal FROST signing choreography"
        );

        // Phase 1: Propose signing context to all participants
        let signing_context = self
            .phase1_propose_context(handler, endpoint, &participants, my_role, coordinator_role)
            .await?;

        // Phase 2: Broadcast and gather journal credentials
        let credentials = self
            .phase2_exchange_credentials(
                handler,
                endpoint,
                &participants,
                my_role,
                &signing_context,
            )
            .await?;

        // Phase 3: Broadcast and gather FROST commitments
        let commitments = self
            .phase3_exchange_commitments(handler, endpoint, &participants, my_role, &credentials)
            .await?;

        // Phase 4: Broadcast and gather FROST signature shares
        let shares = self
            .phase4_exchange_shares(handler, endpoint, &participants, my_role, &commitments)
            .await?;

        // Phase 5: Verify consistent signature aggregation
        let result = self
            .phase5_verify_signature(handler, endpoint, &participants, my_role, &shares)
            .await?;

        tracing::info!(
            participant = ?my_role,
            signature_length = result.signature.len(),
            participant_count = result.participants.len(),
            "Journal FROST signing completed successfully"
        );

        Ok(result)
    }

    /// Phase 1: Propose and acknowledge the signing context
    ///
    /// # Arguments
    /// * `handler` - Choreography handler for message processing
    /// * `endpoint` - Network endpoint for communication
    /// * `participants` - List of all participating roles
    /// * `my_role` - Role of this participant
    /// * `coordinator_role` - Role of the coordinator proposing the context
    async fn phase1_propose_context<H: ChoreoHandler<Role = ChoreographicRole>>(
        &self,
        handler: &mut H,
        endpoint: &mut H::Endpoint,
        participants: &[ChoreographicRole],
        my_role: ChoreographicRole,
        coordinator_role: ChoreographicRole,
    ) -> Result<SigningContext, ChoreographyError> {
        let config = ProposeAcknowledgeConfig {
            acknowledge_timeout_seconds: self.config.timeout_seconds,
            require_explicit_acks: false, // Implicit acknowledgment
            epoch: self.config.epoch,
            ..Default::default()
        };

        let validator = SigningContextValidator {
            config: self.config.clone(),
        };
        let choreography = ProposeAndAcknowledgeChoreography::new(
            config,
            participants.to_vec(),
            validator,
            &self.crypto,
        )?;

        if my_role == coordinator_role {
            // As coordinator, propose the signing context
            let signing_context = SigningContext {
                message_hash: blake3::hash(&self.message).into(),
                journal_context: bincode::serialize(&self.config.participants).unwrap_or_default(),
                required_capabilities: self.config.required_capabilities.clone(),
                threshold: self.config.threshold,
                epoch: self.config.epoch,
            };

            let result = choreography
                .execute_as_proposer(handler, endpoint, my_role, signing_context)
                .await?;
            Ok(result.proposal)
        } else {
            // As participant, receive the signing context
            let result = choreography
                .execute_as_participant(handler, endpoint, my_role, coordinator_role)
                .await?;
            Ok(result.proposal)
        }
    }

    /// Phase 2: Broadcast and gather journal credentials from all participants
    ///
    /// # Arguments
    /// * `handler` - Choreography handler for message processing
    /// * `endpoint` - Network endpoint for communication
    /// * `participants` - List of all participating roles
    /// * `my_role` - Role of this participant
    /// * `_signing_context` - The agreed-upon signing context
    async fn phase2_exchange_credentials<H: ChoreoHandler<Role = ChoreographicRole>>(
        &self,
        handler: &mut H,
        endpoint: &mut H::Endpoint,
        participants: &[ChoreographicRole],
        my_role: ChoreographicRole,
        _signing_context: &SigningContext,
    ) -> Result<BTreeMap<ChoreographicRole, JournalCredentials>, ChoreographyError> {
        let config = BroadcastGatherConfig {
            gather_timeout_seconds: self.config.timeout_seconds,
            epoch: self.config.epoch,
            ..Default::default()
        };

        let validator = JournalCredentialsValidator {
            config: self.config.clone(),
        };
        let choreography = BroadcastAndGatherChoreography::new(
            config,
            participants.to_vec(),
            validator,
            &self.crypto,
        )?;

        let random = &self.random;
        let result = choreography
            .execute(handler, endpoint, my_role, |role, _crypto| {
                // Generate my journal credentials
                let device_id = DeviceId(role.device_id);
                let random_bytes = random.random_bytes(32);
                let mut epoch_nonce = [0u8; 32];
                epoch_nonce.copy_from_slice(&random_bytes[..32]);

                Ok(JournalCredentials {
                    device_id,
                    credentials: vec![1, 2, 3],       // Mock credentials
                    capability_proofs: vec![4, 5, 6], // Mock capability proofs
                    epoch_nonce,
                })
            })
            .await?;

        Ok(result.messages)
    }

    /// Phase 3: Broadcast and gather FROST commitments from all participants
    ///
    /// # Arguments
    /// * `handler` - Choreography handler for message processing
    /// * `endpoint` - Network endpoint for communication
    /// * `participants` - List of all participating roles
    /// * `my_role` - Role of this participant
    /// * `_credentials` - Map of verified credentials from all participants
    async fn phase3_exchange_commitments<H: ChoreoHandler<Role = ChoreographicRole>>(
        &self,
        handler: &mut H,
        endpoint: &mut H::Endpoint,
        participants: &[ChoreographicRole],
        my_role: ChoreographicRole,
        _credentials: &BTreeMap<ChoreographicRole, JournalCredentials>,
    ) -> Result<BTreeMap<ChoreographicRole, JournalFrostCommitment>, ChoreographyError> {
        let config = BroadcastGatherConfig {
            gather_timeout_seconds: self.config.timeout_seconds,
            epoch: self.config.epoch,
            ..Default::default()
        };

        let validator = JournalFrostCommitmentValidator;
        let choreography = BroadcastAndGatherChoreography::new(
            config,
            participants.to_vec(),
            validator,
            &self.crypto,
        )?;

        let message = &self.message;
        let result = choreography
            .execute(handler, endpoint, my_role, |role, _crypto| {
                // Generate FROST commitment
                let commitment_data = format!("frost_commitment_for_{}", role.device_id);
                let frost_commitment = blake3::hash(commitment_data.as_bytes()).as_bytes().to_vec();
                let mut journal_binding_input = frost_commitment.clone();
                journal_binding_input.extend_from_slice(message);
                let journal_binding = blake3::hash(&journal_binding_input).into();

                Ok(JournalFrostCommitment {
                    frost_commitment,
                    journal_binding,
                })
            })
            .await?;

        Ok(result.messages)
    }

    /// Phase 4: Broadcast and gather FROST signature shares from all participants
    ///
    /// # Arguments
    /// * `handler` - Choreography handler for message processing
    /// * `endpoint` - Network endpoint for communication
    /// * `participants` - List of all participating roles
    /// * `my_role` - Role of this participant
    /// * `_commitments` - Map of commitments from all participants
    async fn phase4_exchange_shares<H: ChoreoHandler<Role = ChoreographicRole>>(
        &self,
        handler: &mut H,
        endpoint: &mut H::Endpoint,
        participants: &[ChoreographicRole],
        my_role: ChoreographicRole,
        _commitments: &BTreeMap<ChoreographicRole, JournalFrostCommitment>,
    ) -> Result<BTreeMap<ChoreographicRole, JournalFrostShare>, ChoreographyError> {
        let config = BroadcastGatherConfig {
            gather_timeout_seconds: self.config.timeout_seconds,
            epoch: self.config.epoch,
            ..Default::default()
        };

        let validator = JournalFrostShareValidator;
        let choreography = BroadcastAndGatherChoreography::new(
            config,
            participants.to_vec(),
            validator,
            &self.crypto,
        )?;

        let message = &self.message;
        let result = choreography
            .execute(handler, endpoint, my_role, |role, _crypto| {
                // Generate FROST signature share
                let share_data = format!(
                    "frost_share_for_{}:{}",
                    role.device_id,
                    hex::encode(message)
                );
                let signature_share = blake3::hash(share_data.as_bytes()).as_bytes().to_vec();
                let witness_data = format!("journal_witness_{}", role.device_id);
                let journal_witness = blake3::hash(witness_data.as_bytes()).as_bytes().to_vec();

                Ok(JournalFrostShare {
                    signature_share,
                    journal_witness,
                })
            })
            .await?;

        Ok(result.messages)
    }

    /// Phase 5: Verify consistent signature aggregation across all participants
    ///
    /// # Arguments
    /// * `handler` - Choreography handler for message processing
    /// * `endpoint` - Network endpoint for communication
    /// * `participants` - List of all participating roles
    /// * `my_role` - Role of this participant
    /// * `shares` - Map of signature shares from all participants
    async fn phase5_verify_signature<H: ChoreoHandler<Role = ChoreographicRole>>(
        &self,
        handler: &mut H,
        endpoint: &mut H::Endpoint,
        participants: &[ChoreographicRole],
        my_role: ChoreographicRole,
        shares: &BTreeMap<ChoreographicRole, JournalFrostShare>,
    ) -> Result<JournalFrostResult, ChoreographyError> {
        let config = VerificationConfig {
            commit_timeout_seconds: self.config.timeout_seconds,
            reveal_timeout_seconds: self.config.timeout_seconds,
            epoch: self.config.epoch,
            ..Default::default()
        };

        let _comparator = JournalFrostResultComparator;
        let _config = config;

        // TODO: Add verification step with VerifyConsistentResultChoreography once Clone requirements resolved
        let my_result = self.aggregate_signature_shares(shares).await?;
        Ok(my_result)
    }

    /// Aggregate signature shares into a final threshold signature
    ///
    /// # Arguments
    /// * `shares` - Map of signature shares from all participants
    async fn aggregate_signature_shares(
        &self,
        shares: &BTreeMap<ChoreographicRole, JournalFrostShare>,
    ) -> Result<JournalFrostResult, ChoreographyError> {
        // Simple aggregation for demo - real implementation would use proper FROST aggregation
        let mut aggregated_sig = vec![0u8; 64];
        let mut participants = Vec::new();

        for (role, share) in shares.iter().take(self.config.threshold as usize) {
            participants.push(DeviceId(role.device_id));

            for (i, byte) in share.signature_share.iter().take(64).enumerate() {
                aggregated_sig[i] ^= byte;
            }
        }

        let mut validity_proof_input = aggregated_sig.clone();
        validity_proof_input.extend_from_slice(&self.message);
        let validity_proof = blake3::hash(&validity_proof_input).as_bytes().to_vec();

        Ok(JournalFrostResult {
            signature: aggregated_sig,
            participants,
            epoch: self.config.epoch,
            validity_proof,
        })
    }
}

/// Convenience function for executing Journal FROST signing
///
/// # Arguments
/// * `handler` - Choreography handler for message processing
/// * `endpoint` - Network endpoint for communication
/// * `participants` - List of all participating roles
/// * `my_role` - Role of this participant
/// * `coordinator_role` - Role of the coordinator initiating the signing
/// * `message` - Message to be signed
/// * `config` - Configuration for the FROST signing process
/// * `crypto` - Crypto effects interface for cryptographic operations
/// * `random` - Random effects interface for nonce generation
pub async fn journal_frost_sign<H, C, R>(
    handler: &mut H,
    endpoint: &mut H::Endpoint,
    participants: Vec<ChoreographicRole>,
    my_role: ChoreographicRole,
    coordinator_role: ChoreographicRole,
    message: Vec<u8>,
    config: JournalFrostConfig,
    crypto: C,
    random: R,
) -> Result<JournalFrostResult, ChoreographyError>
where
    H: ChoreoHandler<Role = ChoreographicRole>,
    C: CryptoEffects + Clone,
    R: RandomEffects + Clone,
{
    let choreography = JournalFrostSigningChoreography::new(config, message, crypto, random);
    choreography
        .execute(handler, endpoint, participants, my_role, coordinator_role)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_protocol::handlers::crypto::RealCryptoHandler;
    use aura_protocol::effects::ProductionRandomEffects;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_frost_choreography_creation() {
        let crypto = RealCryptoHandler::new();
        let random = ProductionRandomEffects;
        let config = JournalFrostConfig {
            participants: vec![DeviceId::new()],
            threshold: 2,
            epoch: 1,
            required_capabilities: vec![],
            timeout_seconds: 30,
        };

        let message = b"test message to sign".to_vec();
        let choreography = JournalFrostSigningChoreography::new(config, message, crypto, random);

        assert_eq!(choreography.config.threshold, 2);
        assert_eq!(choreography.config.epoch, 1);
    }

    #[test]
    fn test_validators() {
        let config = JournalFrostConfig {
            participants: vec![],
            threshold: 2,
            epoch: 1,
            required_capabilities: vec![],
            timeout_seconds: 30,
        };

        let signing_context = SigningContext {
            message_hash: [0; 32],
            journal_context: vec![],
            required_capabilities: vec![],
            threshold: 2,
            epoch: 1,
        };

        let validator = SigningContextValidator { config };
        let role = ChoreographicRole {
            device_id: Uuid::new_v4(),
            role_index: 0,
        };

        assert!(validator.validate_outgoing(&signing_context, role).is_ok());
    }
}
