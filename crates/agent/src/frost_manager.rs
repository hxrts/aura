//! FROST Key Share Management for Agent Layer
//!
//! This module provides high-level FROST operations for the agent layer,
//! including key share management, threshold signing coordination, and
//! key rotation/resharing.

use crate::{AgentError, Result};
use aura_coordination::{
    protocols::dkg::{create_dkg_protocol, DkgOutput},
    session_types::frost::{
        new_session_typed_frost, FrostAggregationOperations, FrostCommitmentOperations, FrostIdle,
        FrostSessionState, FrostSigningOperations, SessionTypedFrost,
    },
};
use aura_crypto::{
    frost::{FrostKeyShare, FrostSigner},
    Effects,
};
use aura_types::{DeviceId, SessionId};
use ed25519_dalek::{Signature, VerifyingKey};
use frost_ed25519 as frost;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;

/// FROST key share storage and management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrostKeyManager {
    /// Device ID for this manager
    device_id: DeviceId,
    /// Current key share (if any)
    key_share: Option<FrostKeyShare>,
    /// Public key package for verification
    public_key_package: Option<frost::keys::PublicKeyPackage>,
    /// Threshold configuration
    threshold: u16,
    /// Total number of participants
    max_participants: u16,
    /// FROST participant identifier
    participant_id: Option<frost::Identifier>,
}

impl FrostKeyManager {
    /// Create a new FROST key manager
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            key_share: None,
            public_key_package: None,
            threshold: 0,
            max_participants: 0,
            participant_id: None,
        }
    }

    /// Initialize with DKG-generated keys
    pub fn initialize_with_dkg(
        &mut self,
        dkg_output: DkgOutput,
        threshold: u16,
        max_participants: u16,
    ) -> Result<()> {
        let frost_key_share = FrostKeyShare {
            identifier: dkg_output.participant_id,
            signing_share: *dkg_output.key_package.signing_share(),
            verifying_key: *dkg_output.public_key_package.verifying_key(),
        };

        self.key_share = Some(frost_key_share);
        self.public_key_package = Some(dkg_output.public_key_package);
        self.threshold = threshold;
        self.max_participants = max_participants;
        self.participant_id = Some(dkg_output.participant_id);

        info!(
            "FROST key manager initialized with {}-of-{} threshold",
            threshold, max_participants
        );

        Ok(())
    }

    /// Get the current key share
    pub fn get_key_share(&self) -> Option<&FrostKeyShare> {
        self.key_share.as_ref()
    }

    /// Get the group public key for verification
    pub fn get_group_public_key(&self) -> Result<VerifyingKey> {
        let public_key_package = self
            .public_key_package
            .as_ref()
            .ok_or_else(|| AgentError::frost_operation_failed("No public key package available"))?;

        let frost_verifying_key = public_key_package.verifying_key();
        aura_crypto::frost::frost_verifying_key_to_dalek(frost_verifying_key).map_err(|e| {
            AgentError::frost_operation_failed(&format!("Key conversion failed: {:?}", e))
        })
    }

    /// Get participant ID
    pub fn get_participant_id(&self) -> Option<frost::Identifier> {
        self.participant_id
    }

    /// Check if this manager has keys and can participate in signing
    pub fn is_ready_for_signing(&self) -> bool {
        self.key_share.is_some()
            && self.public_key_package.is_some()
            && self.participant_id.is_some()
    }

    /// Create a signing session for a message
    pub async fn create_signing_session(
        &self,
        message: &[u8],
        session_id: SessionId,
    ) -> Result<FrostSigningSession> {
        if !self.is_ready_for_signing() {
            return Err(AgentError::frost_operation_failed(
                "FROST key manager not ready for signing (missing keys)",
            ));
        }

        let participant_id = self.participant_id.unwrap();
        let key_share = self.key_share.as_ref().unwrap().clone();

        let session = FrostSigningSession::new(
            self.device_id,
            participant_id,
            key_share,
            self.threshold,
            self.max_participants,
            message.to_vec(),
            session_id,
        );

        debug!(
            "Created FROST signing session {} for participant {:?}",
            session_id.0, participant_id
        );

        Ok(session)
    }

    /// Verify a threshold signature
    pub fn verify_threshold_signature(&self, message: &[u8], signature: &Signature) -> Result<()> {
        let group_public_key = self.get_group_public_key()?;

        aura_crypto::frost::verify_signature(message, signature, &group_public_key).map_err(|e| {
            AgentError::frost_operation_failed(&format!("Signature verification failed: {:?}", e))
        })
    }

    /// Update key share after resharing
    pub fn update_key_share(
        &mut self,
        new_key_share: FrostKeyShare,
        new_public_key_package: frost::keys::PublicKeyPackage,
        new_threshold: u16,
        new_max_participants: u16,
    ) -> Result<()> {
        self.key_share = Some(new_key_share);
        self.public_key_package = Some(new_public_key_package);
        self.threshold = new_threshold;
        self.max_participants = new_max_participants;

        info!(
            "FROST key manager updated with new {}-of-{} threshold",
            new_threshold, new_max_participants
        );

        Ok(())
    }

    /// Get threshold configuration
    pub fn get_threshold_config(&self) -> (u16, u16) {
        (self.threshold, self.max_participants)
    }

    /// Export key share for secure storage
    pub fn export_key_share(&self) -> Result<Vec<u8>> {
        let key_share = self
            .key_share
            .as_ref()
            .ok_or_else(|| AgentError::frost_operation_failed("No key share to export"))?;

        // Use JSON serialization for better compatibility with manual serde implementation
        serde_json::to_vec(key_share)
            .map_err(|e| AgentError::frost_operation_failed(&format!("Key export failed: {:?}", e)))
    }

    /// Import key share from secure storage
    pub fn import_key_share(&mut self, data: &[u8]) -> Result<()> {
        let key_share: FrostKeyShare = serde_json::from_slice(data).map_err(|e| {
            AgentError::frost_operation_failed(&format!("Key import failed: {:?}", e))
        })?;

        self.participant_id = Some(key_share.identifier);
        self.key_share = Some(key_share);

        info!(
            "FROST key share imported for participant {:?}",
            self.participant_id
        );

        Ok(())
    }
}

/// FROST signing session for coordinating threshold signatures
#[derive(Debug)]
pub struct FrostSigningSession {
    device_id: DeviceId,
    session_id: SessionId,
    message: Vec<u8>,
    frost_session: RwLock<FrostSessionState>,
    key_share: FrostKeyShare,
    threshold: u16,
    max_participants: u16,
}

impl FrostSigningSession {
    /// Create a new signing session
    pub fn new(
        device_id: DeviceId,
        participant_id: frost::Identifier,
        key_share: FrostKeyShare,
        threshold: u16,
        max_participants: u16,
        message: Vec<u8>,
        session_id: SessionId,
    ) -> Self {
        let frost_session =
            new_session_typed_frost(device_id, participant_id, threshold, max_participants);

        Self {
            device_id,
            session_id,
            message,
            frost_session: RwLock::new(FrostSessionState::FrostIdle(frost_session)),
            key_share,
            threshold,
            max_participants,
        }
    }

    /// Generate commitment for this session
    pub async fn generate_commitment(&self) -> Result<aura_crypto::SigningCommitment> {
        let mut session_guard = self.frost_session.write().await;

        match &mut *session_guard {
            FrostSessionState::FrostIdle(idle_session) => {
                // Transition to commitment phase
                let commitment_session = idle_session.clone();
                // TODO: Add proper state transition when coordination layer is integrated

                // For now, generate commitment directly
                drop(session_guard);
                self.generate_commitment_direct().await
            }
            _ => Err(AgentError::frost_operation_failed(
                "Session not in idle state for commitment generation",
            )),
        }
    }

    /// Generate commitment directly (temporary implementation)
    async fn generate_commitment_direct(&self) -> Result<aura_crypto::SigningCommitment> {
        use aura_crypto::FrostSigner;

        let effects = Effects::production();
        let mut rng = effects.rng();

        let (nonces, commitments) =
            FrostSigner::generate_nonces(&self.key_share.signing_share, &mut rng);

        Ok(aura_crypto::SigningCommitment {
            identifier: self.key_share.identifier,
            commitment: commitments,
        })
    }

    /// Create signature share after collecting commitments
    pub async fn create_signature_share(
        &self,
        commitments: BTreeMap<frost::Identifier, aura_crypto::SigningCommitment>,
    ) -> Result<aura_crypto::SignatureShare> {
        if commitments.len() < self.threshold as usize {
            return Err(AgentError::frost_operation_failed(&format!(
                "Insufficient commitments: need {}, got {}",
                self.threshold,
                commitments.len()
            )));
        }

        // TODO: Implement proper session state management
        // For now, create signature share directly
        self.create_signature_share_direct(&commitments).await
    }

    /// Create signature share directly (temporary implementation)
    async fn create_signature_share_direct(
        &self,
        commitments: &BTreeMap<frost::Identifier, aura_crypto::SigningCommitment>,
    ) -> Result<aura_crypto::SignatureShare> {
        use aura_crypto::FrostSigner;

        let effects = Effects::production();
        let mut rng = effects.rng();

        // Generate nonces for this signing round
        let (nonces, _) = FrostSigner::generate_nonces(&self.key_share.signing_share, &mut rng);

        // Convert commitments to FROST format
        let mut frost_commitments = BTreeMap::new();
        for (id, commitment) in commitments {
            frost_commitments.insert(*id, commitment.commitment);
        }

        // Generate temporary KeyPackage for signing
        // TODO: Use proper KeyPackage from DKG when coordination is integrated
        let (secret_shares, _) = frost::keys::generate_with_dealer(
            self.threshold,
            self.max_participants,
            frost::keys::IdentifierList::Default,
            &mut rng,
        )
        .map_err(|e| {
            AgentError::frost_operation_failed(&format!("Key generation failed: {:?}", e))
        })?;

        let secret_share = secret_shares
            .values()
            .next()
            .ok_or_else(|| AgentError::frost_operation_failed("No secret share available"))?;

        let key_package = frost::keys::KeyPackage::try_from(secret_share.clone()).map_err(|e| {
            AgentError::frost_operation_failed(&format!("KeyPackage creation failed: {:?}", e))
        })?;

        // Create signature share
        let signature_share = FrostSigner::sign_share_with_package(
            &self.message,
            &nonces,
            &frost_commitments,
            &key_package,
        )
        .map_err(|e| {
            AgentError::frost_operation_failed(&format!("Signature share creation failed: {:?}", e))
        })?;

        Ok(aura_crypto::SignatureShare {
            identifier: self.key_share.identifier,
            share: signature_share,
        })
    }

    /// Aggregate signature shares into final signature
    pub async fn aggregate_signature(
        &self,
        commitments: BTreeMap<frost::Identifier, aura_crypto::SigningCommitment>,
        signature_shares: BTreeMap<frost::Identifier, aura_crypto::SignatureShare>,
    ) -> Result<Signature> {
        if signature_shares.len() < self.threshold as usize {
            return Err(AgentError::frost_operation_failed(&format!(
                "Insufficient signature shares: need {}, got {}",
                self.threshold,
                signature_shares.len()
            )));
        }

        // TODO: Implement proper session state management
        // For now, aggregate directly
        self.aggregate_signature_direct(&commitments, &signature_shares)
            .await
    }

    /// Aggregate signature directly (temporary implementation)
    async fn aggregate_signature_direct(
        &self,
        commitments: &BTreeMap<frost::Identifier, aura_crypto::SigningCommitment>,
        signature_shares: &BTreeMap<frost::Identifier, aura_crypto::SignatureShare>,
    ) -> Result<Signature> {
        use aura_crypto::FrostSigner;

        let effects = Effects::production();
        let mut rng = effects.rng();

        // Generate temporary public key package for aggregation
        let (_, pubkey_package) = frost::keys::generate_with_dealer(
            self.threshold,
            self.max_participants,
            frost::keys::IdentifierList::Default,
            &mut rng,
        )
        .map_err(|e| {
            AgentError::frost_operation_failed(&format!("Key generation failed: {:?}", e))
        })?;

        // Convert to FROST format
        let mut frost_commitments = BTreeMap::new();
        for (id, commitment) in commitments {
            frost_commitments.insert(*id, commitment.commitment);
        }

        let mut frost_shares = BTreeMap::new();
        for (id, share) in signature_shares {
            frost_shares.insert(*id, share.share);
        }

        // Aggregate signature
        FrostSigner::aggregate(
            &self.message,
            &frost_commitments,
            &frost_shares,
            &pubkey_package,
        )
        .map_err(|e| {
            AgentError::frost_operation_failed(&format!("Signature aggregation failed: {:?}", e))
        })
    }

    /// Get session information
    pub fn get_session_info(&self) -> (SessionId, DeviceId, u16, u16) {
        (
            self.session_id,
            self.device_id,
            self.threshold,
            self.max_participants,
        )
    }
}

/// High-level FROST operations for the agent
pub struct FrostAgent {
    key_manager: RwLock<FrostKeyManager>,
    device_id: DeviceId,
}

impl FrostAgent {
    /// Create a new FROST agent
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            key_manager: RwLock::new(FrostKeyManager::new(device_id)),
            device_id,
        }
    }

    /// Initialize keys via DKG
    pub async fn initialize_keys_with_dkg(
        &self,
        threshold: u16,
        participants: Vec<DeviceId>,
    ) -> Result<()> {
        let max_participants = participants.len() as u16;

        // Find our position in the participant list
        let position = participants
            .iter()
            .position(|&id| id == self.device_id)
            .ok_or_else(|| AgentError::frost_operation_failed("Device not in participant list"))?;

        // Create DKG protocol
        let session_id = SessionId::from_uuid(Uuid::new_v4());
        let dkg_protocol = create_dkg_protocol(session_id, self.device_id, threshold, participants)
            .map_err(|e| {
                AgentError::frost_operation_failed(&format!(
                    "DKG protocol creation failed: {:?}",
                    e
                ))
            })?;

        // Implement real distributed key generation (DKG) using coordination layer
        info!(
            "Starting distributed key generation with {} participants",
            participants.len()
        );

        let dkg_result = self
            .run_distributed_dkg(threshold, max_participants, &participants)
            .await?;
        let (shares, pubkey_package) = dkg_result;

        let participant_id = frost::Identifier::try_from((position + 1) as u16)
            .map_err(|_| AgentError::frost_operation_failed("Invalid participant ID"))?;

        let secret_share = shares.get(&participant_id).ok_or_else(|| {
            AgentError::frost_operation_failed("No secret share for this participant")
        })?;

        let key_package = frost::keys::KeyPackage::try_from(secret_share.clone()).map_err(|e| {
            AgentError::frost_operation_failed(&format!("KeyPackage creation failed: {:?}", e))
        })?;

        let dkg_output = DkgOutput {
            key_package,
            public_key_package: pubkey_package,
            participant_id,
        };

        // Initialize key manager
        let mut key_manager = self.key_manager.write().await;
        key_manager.initialize_with_dkg(dkg_output, threshold, max_participants)?;

        info!(
            "FROST keys initialized via DKG for device {}",
            self.device_id
        );

        Ok(())
    }

    /// Create a threshold signature for a message
    pub async fn threshold_sign(&self, message: &[u8]) -> Result<Signature> {
        let key_manager = self.key_manager.read().await;

        if !key_manager.is_ready_for_signing() {
            return Err(AgentError::frost_operation_failed(
                "FROST agent not ready for signing (missing keys)",
            ));
        }

        // For single-device testing, perform complete threshold signing
        let session_id = SessionId::from_uuid(Uuid::new_v4());
        let session = key_manager
            .create_signing_session(message, session_id)
            .await?;

        // Implement real distributed threshold signing using coordination layer
        info!("Starting distributed threshold signing session");

        self.run_distributed_signing(session).await
    }

    /// Verify a threshold signature
    pub async fn verify_threshold_signature(
        &self,
        message: &[u8],
        signature: &Signature,
    ) -> Result<()> {
        let key_manager = self.key_manager.read().await;
        key_manager.verify_threshold_signature(message, signature)
    }

    /// Get current threshold configuration
    pub async fn get_threshold_config(&self) -> Result<(u16, u16)> {
        let key_manager = self.key_manager.read().await;

        if !key_manager.is_ready_for_signing() {
            return Err(AgentError::frost_operation_failed(
                "FROST agent not initialized",
            ));
        }

        Ok(key_manager.get_threshold_config())
    }

    /// Check if agent is ready for FROST operations
    pub async fn is_ready(&self) -> bool {
        let key_manager = self.key_manager.read().await;
        key_manager.is_ready_for_signing()
    }

    /// Export keys for secure storage
    pub async fn export_keys(&self) -> Result<Vec<u8>> {
        let key_manager = self.key_manager.read().await;
        key_manager.export_key_share()
    }

    /// Import keys from secure storage
    pub async fn import_keys(&self, data: &[u8]) -> Result<()> {
        let mut key_manager = self.key_manager.write().await;
        key_manager.import_key_share(data)
    }

    /// Run distributed key generation using coordination layer
    async fn run_distributed_dkg(
        &self,
        threshold: u16,
        max_participants: u16,
        participants: &[DeviceId],
    ) -> Result<(
        BTreeMap<frost::Identifier, frost::keys::SecretShare>,
        frost::keys::PublicKeyPackage,
    )> {
        use aura_coordination::context_builder::ContextBuilder;
        use aura_coordination::execution::context::ExecutionContext;
        use aura_coordination::protocols::dkg::{DkgResult, DkgSession};

        info!(
            "Starting distributed key generation with {} participants",
            participants.len()
        );

        // Create execution context for DKG protocol
        let context = ContextBuilder::new(self.device_id)
            .with_session_id(SessionId::from_uuid(uuid::Uuid::new_v4()))
            .with_participants(participants.to_vec())
            .build()
            .map_err(|e| {
                AgentError::frost_operation_failed(&format!("Context creation failed: {:?}", e))
            })?;

        // Create DKG session
        let dkg_session = DkgSession::new(
            context,
            threshold,
            max_participants,
            participants.to_vec(),
        )
        .map_err(|e| {
            AgentError::frost_operation_failed(&format!("DKG session creation failed: {:?}", e))
        })?;

        // Execute DKG protocol through coordination layer
        match dkg_session.execute().await {
            Ok(DkgResult::Success {
                secret_shares,
                public_key_package,
            }) => {
                info!("DKG protocol completed successfully");
                Ok((secret_shares, public_key_package))
            }
            Ok(DkgResult::Failed { error }) => Err(AgentError::frost_operation_failed(&format!(
                "DKG protocol failed: {}",
                error
            ))),
            Err(e) => Err(AgentError::frost_operation_failed(&format!(
                "DKG execution error: {:?}",
                e
            ))),
        }
    }

    /// Run distributed threshold signing using coordination layer
    async fn run_distributed_signing(&self, session: FrostSigningSession) -> Result<Signature> {
        use aura_coordination::context_builder::ContextBuilder;
        use aura_coordination::protocols::threshold_signing::{
            SigningResult, ThresholdSigningSession,
        };

        let (session_id, device_id, threshold, max_participants) = session.get_session_info();

        info!(
            "Starting distributed threshold signing session {}",
            session_id.0
        );

        // Create execution context for threshold signing
        let context = ContextBuilder::new(device_id)
            .with_session_id(session_id)
            .with_threshold(threshold)
            .build()
            .map_err(|e| {
                AgentError::frost_operation_failed(&format!(
                    "Signing context creation failed: {:?}",
                    e
                ))
            })?;

        // Create threshold signing session
        let signing_session = ThresholdSigningSession::new(context, session).map_err(|e| {
            AgentError::frost_operation_failed(&format!("Signing session creation failed: {:?}", e))
        })?;

        // Execute threshold signing protocol
        match signing_session.execute().await {
            Ok(SigningResult::Success { signature }) => {
                info!("Threshold signing completed successfully");
                Ok(signature)
            }
            Ok(SigningResult::Failed { error }) => Err(AgentError::frost_operation_failed(
                &format!("Threshold signing failed: {}", error),
            )),
            Err(e) => Err(AgentError::frost_operation_failed(&format!(
                "Signing execution error: {:?}",
                e
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_types::DeviceIdExt;

    #[tokio::test]
    async fn test_frost_key_manager_creation() {
        let device_id = DeviceId::new();
        let manager = FrostKeyManager::new(device_id);

        assert!(!manager.is_ready_for_signing());
        assert!(manager.get_key_share().is_none());
    }

    #[tokio::test]
    async fn test_frost_agent_creation() {
        let device_id = DeviceId::new();
        let agent = FrostAgent::new(device_id);

        assert!(!agent.is_ready().await);
    }

    #[tokio::test]
    async fn test_frost_agent_key_initialization() {
        let device_id = DeviceId::new();
        let agent = FrostAgent::new(device_id);

        let participants = vec![device_id];
        let result = agent.initialize_keys_with_dkg(1, participants).await;

        assert!(result.is_ok());
        assert!(agent.is_ready().await);
    }

    #[tokio::test]
    async fn test_threshold_signing() {
        let device_id = DeviceId::new();
        let agent = FrostAgent::new(device_id);

        // Initialize with 1-of-1 for testing
        let participants = vec![device_id];
        agent
            .initialize_keys_with_dkg(1, participants)
            .await
            .unwrap();

        let message = b"test message for threshold signing";
        let signature = agent.threshold_sign(message).await.unwrap();

        // Verify signature
        let verification = agent.verify_threshold_signature(message, &signature).await;
        assert!(verification.is_ok());
    }
}
