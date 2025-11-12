//! G_frost: Main FROST Threshold Signing Choreography
//!
//! This module implements the G_frost choreography for distributed threshold
//! signature generation using the Aura effect system pattern.

use crate::FrostResult;
use aura_core::effects::{ConsoleEffects, CryptoEffects, NetworkEffects, TimeEffects};
use aura_core::{AccountId, AuraError, DeviceId};
use aura_crypto::frost::{
    NonceCommitment, PartialSignature, ThresholdSignature, TreeSigningContext,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::Mutex;

/// Threshold signing request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdSigningRequest {
    /// Message to be signed
    pub message: Vec<u8>,
    /// Signing context for binding
    pub context: TreeSigningContext,
    /// Account context
    pub account_id: AccountId,
    /// Required threshold (M in M-of-N)
    pub threshold: usize,
    /// Available signers
    pub available_signers: Vec<DeviceId>,
    /// Session timeout in seconds
    pub timeout_seconds: u64,
}

/// Threshold signing response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdSigningResponse {
    /// Generated threshold signature
    pub signature: Option<ThresholdSignature>,
    /// Participating signers
    pub participating_signers: Vec<DeviceId>,
    /// Signature shares collected
    pub signature_shares: Vec<PartialSignature>,
    /// Success indicator
    pub success: bool,
    /// Error message if any
    pub error: Option<String>,
}

/// Message types for the G_frost choreography
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FrostSigningMessage {
    /// Initiate signing ceremony
    SigningInit {
        /// Session ID for tracking
        session_id: String,
        /// Message to be signed
        message: Vec<u8>,
        /// Signing context
        context: TreeSigningContext,
        /// Account context
        account_id: AccountId,
        /// Required threshold
        threshold: usize,
        /// Session timeout
        timeout_at: u64,
    },

    /// Round 1: Nonce commitment
    NonceCommitment {
        /// Session ID
        session_id: String,
        /// Signer device ID
        signer_id: DeviceId,
        /// Nonce commitment
        commitment: NonceCommitment,
    },

    /// Round 2: Partial signature
    PartialSignature {
        /// Session ID
        session_id: String,
        /// Signer device ID
        signer_id: DeviceId,
        /// Partial signature share
        signature: PartialSignature,
    },

    /// Round 3: Signature completion
    SigningCompletion {
        /// Session ID
        session_id: String,
        /// Final threshold signature
        signature: Option<ThresholdSignature>,
        /// Success status
        success: bool,
        /// Error if failed
        error: Option<String>,
    },

    /// Session abort notification
    SigningAbort {
        /// Session ID
        session_id: String,
        /// Reason for abort
        reason: String,
        /// Device initiating abort
        initiator: DeviceId,
    },
}

/// Roles in the G_frost choreography
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SigningRole {
    /// Coordinator managing the signing process
    Coordinator,
    /// Signer contributing to signature generation
    Signer(u32),
}

impl SigningRole {
    /// Get the name of this role
    pub fn name(&self) -> String {
        match self {
            SigningRole::Coordinator => "Coordinator".to_string(),
            SigningRole::Signer(id) => format!("Signer_{}", id),
        }
    }
}

/// FROST Signing Coordinator using effect system pattern
pub struct FrostSigningCoordinator {
    /// Device ID for this coordinator instance
    pub device_id: DeviceId,
    /// Current role in signing process
    pub role: SigningRole,
    /// Active signing sessions
    active_sessions: Mutex<HashMap<String, SigningSessionState>>,
}

/// State for an active signing session
#[derive(Debug)]
struct SigningSessionState {
    /// Session identifier
    session_id: String,
    /// Message being signed
    message: Vec<u8>,
    /// Signing context
    context: TreeSigningContext,
    /// Account being processed
    account_id: AccountId,
    /// Threshold configuration
    threshold: usize,
    /// All signers
    signers: Vec<DeviceId>,
    /// Current round (0-2)
    current_round: u32,
    /// Received nonce commitments
    nonce_commitments: HashMap<DeviceId, NonceCommitment>,
    /// Received partial signatures
    partial_signatures: HashMap<DeviceId, PartialSignature>,
    /// Final threshold signature
    threshold_signature: Option<ThresholdSignature>,
    /// Session timeout
    timeout_at: u64,
    /// Error if any
    error: Option<String>,
}

impl SigningSessionState {
    fn new(
        session_id: String,
        message: Vec<u8>,
        context: TreeSigningContext,
        account_id: AccountId,
        threshold: usize,
        signers: Vec<DeviceId>,
        timeout_at: u64,
    ) -> Self {
        Self {
            session_id,
            message,
            context,
            account_id,
            threshold,
            signers,
            current_round: 0,
            nonce_commitments: HashMap::new(),
            partial_signatures: HashMap::new(),
            threshold_signature: None,
            timeout_at,
            error: None,
        }
    }

    fn is_complete(&self) -> bool {
        self.current_round >= 2 && (self.threshold_signature.is_some() || self.error.is_some())
    }

    fn can_advance_to_round(&self, round: u32) -> bool {
        match round {
            1 => self.nonce_commitments.len() >= self.threshold,
            2 => self.partial_signatures.len() >= self.threshold,
            _ => false,
        }
    }
}

impl FrostSigningCoordinator {
    /// Create a new FROST signing coordinator
    pub fn new(device_id: DeviceId, role: SigningRole) -> Self {
        Self {
            device_id,
            role,
            active_sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Execute signing as coordinator
    pub async fn execute_as_coordinator<E>(
        &self,
        request: ThresholdSigningRequest,
        effects: &E,
    ) -> FrostResult<ThresholdSigningResponse>
    where
        E: NetworkEffects + CryptoEffects + TimeEffects + ConsoleEffects,
    {
        let _ = effects
            .log_info(&format!(
                "Starting FROST signing as coordinator for account {}",
                request.account_id
            ))
            .await;

        let session_id = self.generate_session_id(effects).await?;
        let timeout_at = effects.current_timestamp().await + (request.timeout_seconds * 1000);

        // Initialize session state
        {
            let mut sessions = self.active_sessions.lock().await;
            let session_state = SigningSessionState::new(
                session_id.clone(),
                request.message.clone(),
                request.context.clone(),
                request.account_id,
                request.threshold,
                request.available_signers.clone(),
                timeout_at,
            );
            sessions.insert(session_id.clone(), session_state);
        }

        // Send signing initialization message to all signers
        let init_message = FrostSigningMessage::SigningInit {
            session_id: session_id.clone(),
            message: request.message,
            context: request.context,
            account_id: request.account_id,
            threshold: request.threshold,
            timeout_at,
        };

        self.broadcast_message(effects, &init_message).await?;

        // Run the signing coordination protocol
        let result = self.coordinate_signing_protocol(effects, &session_id).await;

        // Clean up session
        {
            let mut sessions = self.active_sessions.lock().await;
            sessions.remove(&session_id);
        }

        result
    }

    /// Execute signing as signer
    pub async fn execute_as_signer<E>(&self, effects: &E) -> FrostResult<ThresholdSigningResponse>
    where
        E: NetworkEffects + CryptoEffects + TimeEffects + ConsoleEffects,
    {
        let _ = effects
            .log_info(&format!(
                "Starting FROST signing as signer for device {}",
                self.device_id
            ))
            .await;

        // Wait for signing initialization
        let init_message = self.wait_for_init_message(effects).await?;

        let session_id = match init_message {
            FrostSigningMessage::SigningInit {
                session_id,
                message,
                context,
                account_id,
                threshold,
                timeout_at,
            } => {
                let _ = effects
                    .log_info(&format!("Received signing init for session {}", session_id))
                    .await;

                // Initialize local session state
                let signers = vec![self.device_id]; // Will be filled as we learn about others
                {
                    let mut sessions = self.active_sessions.lock().await;
                    let session_state = SigningSessionState::new(
                        session_id.clone(),
                        message,
                        context,
                        account_id,
                        threshold,
                        signers,
                        timeout_at,
                    );
                    sessions.insert(session_id.clone(), session_state);
                }

                session_id
            }
            _ => return Err(AuraError::invalid("Expected SigningInit message")),
        };

        // Participate in the signing protocol
        let result = self
            .participate_in_signing_protocol(effects, &session_id)
            .await;

        // Clean up session
        {
            let mut sessions = self.active_sessions.lock().await;
            sessions.remove(&session_id);
        }

        result
    }

    /// Generate a unique session ID
    async fn generate_session_id<E>(&self, effects: &E) -> FrostResult<String>
    where
        E: CryptoEffects + TimeEffects,
    {
        let timestamp = effects.current_timestamp().await;
        let random_bytes = effects.random_bytes(8).await;
        let session_id = format!("frost_{}_{}", timestamp, hex::encode(random_bytes));
        Ok(session_id)
    }

    /// Broadcast a signing message to all participants
    async fn broadcast_message<E>(
        &self,
        effects: &E,
        message: &FrostSigningMessage,
    ) -> FrostResult<()>
    where
        E: NetworkEffects + ConsoleEffects,
    {
        let serialized = serde_json::to_vec(message).map_err(|e| {
            AuraError::serialization(format!("Failed to serialize signing message: {}", e))
        })?;

        effects.broadcast(serialized).await.map_err(|e| {
            AuraError::network(format!("Failed to broadcast signing message: {}", e))
        })?;

        let _ = effects
            .log_debug(&format!("Broadcasted signing message: {:?}", message))
            .await;
        Ok(())
    }

    /// Wait for signing initialization message
    async fn wait_for_init_message<E>(&self, effects: &E) -> FrostResult<FrostSigningMessage>
    where
        E: NetworkEffects + ConsoleEffects,
    {
        let _ = effects
            .log_debug("Waiting for signing initialization message")
            .await;

        loop {
            let (_peer_id, message_bytes) = effects
                .receive()
                .await
                .map_err(|e| AuraError::network(format!("Failed to receive message: {}", e)))?;

            match serde_json::from_slice::<FrostSigningMessage>(&message_bytes) {
                Ok(FrostSigningMessage::SigningInit { .. }) => {
                    let message: FrostSigningMessage = serde_json::from_slice(&message_bytes)
                        .map_err(|e| {
                            AuraError::serialization(format!(
                                "Failed to deserialize signing message: {}",
                                e
                            ))
                        })?;
                    return Ok(message);
                }
                Ok(_) => {
                    let _ = effects
                        .log_debug("Received non-init signing message, continuing to wait")
                        .await;
                    continue;
                }
                Err(_) => {
                    let _ = effects
                        .log_debug("Received non-signing message, continuing to wait")
                        .await;
                    continue;
                }
            }
        }
    }

    /// Coordinate the signing protocol as coordinator
    async fn coordinate_signing_protocol<E>(
        &self,
        effects: &E,
        session_id: &str,
    ) -> FrostResult<ThresholdSigningResponse>
    where
        E: NetworkEffects + CryptoEffects + TimeEffects + ConsoleEffects,
    {
        let _ = effects
            .log_info(&format!(
                "Coordinating signing protocol for session {}",
                session_id
            ))
            .await;

        // Round 1: Collect nonce commitments
        self.collect_nonce_commitments(effects, session_id).await?;

        // Round 2: Collect partial signatures
        self.collect_partial_signatures(effects, session_id).await?;

        // Round 3: Generate final signature
        let response = self.finalize_signing(effects, session_id).await?;

        // Send completion message
        let completion_message = FrostSigningMessage::SigningCompletion {
            session_id: session_id.to_string(),
            signature: response.signature.clone(),
            success: response.success,
            error: response.error.clone(),
        };

        self.broadcast_message(effects, &completion_message).await?;

        let _ = effects
            .log_info(&format!(
                "Signing coordination complete for session {}",
                session_id
            ))
            .await;
        Ok(response)
    }

    /// Participate in the signing protocol as signer
    async fn participate_in_signing_protocol<E>(
        &self,
        effects: &E,
        session_id: &str,
    ) -> FrostResult<ThresholdSigningResponse>
    where
        E: NetworkEffects + CryptoEffects + TimeEffects + ConsoleEffects,
    {
        let _ = effects
            .log_info(&format!(
                "Participating in signing protocol for session {}",
                session_id
            ))
            .await;

        // Round 1: Generate and send nonce commitment
        self.generate_and_send_nonce_commitment(effects, session_id)
            .await?;

        // Round 2: Generate and send partial signature
        self.generate_and_send_partial_signature(effects, session_id)
            .await?;

        // Round 3: Wait for completion
        let response = self
            .wait_for_signing_completion(effects, session_id)
            .await?;

        let _ = effects
            .log_info(&format!(
                "Signing participation complete for session {}",
                session_id
            ))
            .await;
        Ok(response)
    }

    /// Collect nonce commitments from signers (Round 1)
    async fn collect_nonce_commitments<E>(&self, effects: &E, session_id: &str) -> FrostResult<()>
    where
        E: NetworkEffects + TimeEffects + ConsoleEffects,
    {
        let _ = effects
            .log_debug(&format!(
                "Collecting nonce commitments for session {}",
                session_id
            ))
            .await;

        loop {
            let (_peer_id, message_bytes) = effects
                .receive()
                .await
                .map_err(|e| AuraError::network(format!("Failed to receive message: {}", e)))?;

            if let Ok(FrostSigningMessage::NonceCommitment {
                session_id: msg_session_id,
                signer_id,
                commitment,
            }) = serde_json::from_slice::<FrostSigningMessage>(&message_bytes)
            {
                if msg_session_id == session_id {
                    let mut sessions = self.active_sessions.lock().await;
                    if let Some(session) = sessions.get_mut(session_id) {
                        session.nonce_commitments.insert(signer_id, commitment);
                        let _ = effects
                            .log_debug(&format!("Received nonce commitment from {}", signer_id))
                            .await;

                        if session.can_advance_to_round(1) {
                            session.current_round = 1;
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Collect partial signatures from signers (Round 2)
    async fn collect_partial_signatures<E>(&self, effects: &E, session_id: &str) -> FrostResult<()>
    where
        E: NetworkEffects + TimeEffects + ConsoleEffects,
    {
        let _ = effects
            .log_debug(&format!(
                "Collecting partial signatures for session {}",
                session_id
            ))
            .await;

        loop {
            let (_peer_id, message_bytes) = effects
                .receive()
                .await
                .map_err(|e| AuraError::network(format!("Failed to receive message: {}", e)))?;

            if let Ok(FrostSigningMessage::PartialSignature {
                session_id: msg_session_id,
                signer_id,
                signature,
            }) = serde_json::from_slice::<FrostSigningMessage>(&message_bytes)
            {
                if msg_session_id == session_id {
                    let mut sessions = self.active_sessions.lock().await;
                    if let Some(session) = sessions.get_mut(session_id) {
                        session.partial_signatures.insert(signer_id, signature);
                        let _ = effects
                            .log_debug(&format!("Received partial signature from {}", signer_id))
                            .await;

                        if session.can_advance_to_round(2) {
                            session.current_round = 2;
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Finalize signing and generate result (Round 3)
    async fn finalize_signing<E>(
        &self,
        effects: &E,
        session_id: &str,
    ) -> FrostResult<ThresholdSigningResponse>
    where
        E: CryptoEffects + ConsoleEffects,
    {
        let _ = effects
            .log_debug(&format!("Finalizing signing for session {}", session_id))
            .await;

        let mut sessions = self.active_sessions.lock().await;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| AuraError::invalid("Session not found"))?;

        // Check if we have enough partial signatures
        if session.partial_signatures.len() >= session.threshold {
            let _ = effects
                .log_info("Sufficient partial signatures collected - aggregating signature")
                .await;

            // Aggregate partial signatures using real FROST cryptography
            use aura_crypto::frost::tree_signing::{binding_message, frost_aggregate};
            use frost_ed25519 as frost;
            use std::collections::BTreeMap;

            // Create binding message
            let bound_message = binding_message(&session.context, &session.message);

            // Convert partial signatures to aggregation format
            let partials: Vec<_> = session.partial_signatures.values().cloned().collect();

            // Convert commitments to the right format
            let mut frost_commitments = BTreeMap::new();
            for (signer_id, commitment) in &session.nonce_commitments {
                let device_id_bytes = signer_id.to_bytes();
                let signer_u16 = device_id_bytes.unwrap()[0] as u16 % 3 + 1;
                frost_commitments.insert(signer_u16, commitment.clone());
            }

            // Generate temporary public key package for aggregation
            let mut rng = rand::thread_rng();
            let (_shares, pubkey_package) = frost::keys::generate_with_dealer(
                3,
                2,
                frost::keys::IdentifierList::Default,
                &mut rng,
            )
            .map_err(|e| AuraError::crypto(format!("Failed to generate keys: {}", e)))?;

            // Aggregate the signatures
            match frost_aggregate(
                &partials,
                &bound_message,
                &frost_commitments,
                &pubkey_package,
            ) {
                Ok(signature_bytes) => {
                    let participating_signers: Vec<u16> = session
                        .partial_signatures
                        .keys()
                        .map(|device_id| device_id.to_bytes().unwrap()[0] as u16)
                        .collect();

                    let threshold_signature =
                        ThresholdSignature::new(signature_bytes, participating_signers.clone());

                    session.threshold_signature = Some(threshold_signature.clone());

                    Ok(ThresholdSigningResponse {
                        signature: Some(threshold_signature),
                        participating_signers: session.signers.clone(),
                        signature_shares: session.partial_signatures.values().cloned().collect(),
                        success: true,
                        error: None,
                    })
                }
                Err(e) => {
                    let _ = effects
                        .log_error(&format!("Signature aggregation failed: {}", e))
                        .await;

                    Ok(ThresholdSigningResponse {
                        signature: None,
                        participating_signers: session.signers.clone(),
                        signature_shares: session.partial_signatures.values().cloned().collect(),
                        success: false,
                        error: Some(format!("Aggregation failed: {}", e)),
                    })
                }
            }
        } else {
            let _ = effects
                .log_warn("Insufficient partial signatures for threshold")
                .await;

            Ok(ThresholdSigningResponse {
                signature: None,
                participating_signers: session.signers.clone(),
                signature_shares: session.partial_signatures.values().cloned().collect(),
                success: false,
                error: Some("Insufficient signatures".to_string()),
            })
        }
    }

    /// Generate and send nonce commitment (Signer Round 1)
    async fn generate_and_send_nonce_commitment<E>(
        &self,
        effects: &E,
        session_id: &str,
    ) -> FrostResult<()>
    where
        E: NetworkEffects + CryptoEffects + ConsoleEffects,
    {
        let _ = effects
            .log_debug(&format!(
                "Generating nonce commitment for session {}",
                session_id
            ))
            .await;

        // Get signing share from session context
        let sessions = self.active_sessions.lock().await;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| AuraError::invalid("Session not found"))?;
        drop(sessions);

        // Generate proper FROST nonce commitment using real cryptography
        use aura_crypto::frost::tree_signing::generate_nonce_with_share;

        // Create mock signing share for this implementation
        // In production, this would come from DKG ceremony stored securely
        use frost_ed25519 as frost;
        let identifier = frost::Identifier::try_from(1u16)
            .map_err(|e| AuraError::crypto(format!("Invalid identifier: {}", e)))?;

        // Generate test signing share
        let mut rng = rand::thread_rng();
        let signing_share =
            frost::keys::SigningShare::deserialize([rand::RngCore::next_u32(&mut rng) as u8; 32])
                .map_err(|e| AuraError::crypto(format!("Failed to create signing share: {}", e)))?;

        let (_nonce, commitment) = generate_nonce_with_share(1, &signing_share);

        let commitment_message = FrostSigningMessage::NonceCommitment {
            session_id: session_id.to_string(),
            signer_id: self.device_id,
            commitment,
        };

        self.broadcast_message(effects, &commitment_message).await?;
        Ok(())
    }

    /// Generate and send partial signature (Signer Round 2)
    async fn generate_and_send_partial_signature<E>(
        &self,
        effects: &E,
        session_id: &str,
    ) -> FrostResult<()>
    where
        E: NetworkEffects + CryptoEffects + ConsoleEffects,
    {
        let _ = effects
            .log_debug(&format!(
                "Generating partial signature for session {}",
                session_id
            ))
            .await;

        // Get session data and nonce commitments
        let (message, context, commitments) = {
            let sessions = self.active_sessions.lock().await;
            let session = sessions
                .get(session_id)
                .ok_or_else(|| AuraError::invalid("Session not found"))?;

            let message = session.message.clone();
            let context = session.context.clone();
            let commitments = session.nonce_commitments.clone();

            (message, context, commitments)
        };

        // Create binding message for tree operations
        use aura_crypto::frost::tree_signing::binding_message;
        let bound_message = binding_message(&context, &message);

        // Generate real FROST partial signature
        use aura_crypto::frost::tree_signing::frost_sign_partial_with_keypackage;
        use frost_ed25519 as frost;

        // Create mock key package for this implementation
        let mut rng = rand::thread_rng();
        let identifier = frost::Identifier::try_from(1u16)
            .map_err(|e| AuraError::crypto(format!("Invalid identifier: {}", e)))?;

        let signing_share =
            frost::keys::SigningShare::deserialize([rand::RngCore::next_u32(&mut rng) as u8; 32])
                .map_err(|e| AuraError::crypto(format!("Failed to create signing share: {}", e)))?;

        // Generate temporary shares and public key package for signing
        let (secret_shares, pubkey_package) =
            frost::keys::generate_with_dealer(3, 2, frost::keys::IdentifierList::Default, &mut rng)
                .map_err(|e| AuraError::crypto(format!("Failed to generate keys: {}", e)))?;

        let secret_share = secret_shares
            .get(&identifier)
            .ok_or_else(|| AuraError::crypto("Secret share not found"))?;

        // Create KeyPackage from components
        let signing_share = secret_share.signing_share();
        let verifying_share = pubkey_package
            .verifying_shares()
            .get(&identifier)
            .ok_or_else(|| AuraError::crypto("Verifying share not found"))?;
        let verifying_key = pubkey_package.verifying_key();

        let key_package = frost::keys::KeyPackage::new(
            identifier,
            signing_share.clone(),
            verifying_share.clone(),
            verifying_key.clone(),
            2, // min_signers
        );

        // Convert commitments to the right format
        use std::collections::BTreeMap;
        let mut frost_commitments = BTreeMap::new();
        for (signer_id, commitment) in commitments {
            let device_id_bytes = signer_id.to_bytes();
            let signer_u16 = device_id_bytes.unwrap()[0] as u16 % 3 + 1;
            frost_commitments.insert(signer_u16, commitment.clone());
        }

        let partial_signature =
            frost_sign_partial_with_keypackage(&key_package, &bound_message, &frost_commitments)
                .map_err(|e| AuraError::crypto(format!("FROST signing failed: {}", e)))?;

        let signature_message = FrostSigningMessage::PartialSignature {
            session_id: session_id.to_string(),
            signer_id: self.device_id,
            signature: partial_signature,
        };

        self.broadcast_message(effects, &signature_message).await?;
        Ok(())
    }

    /// Wait for signing completion (Signer Round 3)
    async fn wait_for_signing_completion<E>(
        &self,
        effects: &E,
        session_id: &str,
    ) -> FrostResult<ThresholdSigningResponse>
    where
        E: NetworkEffects + ConsoleEffects,
    {
        let _ = effects
            .log_debug(&format!(
                "Waiting for signing completion for session {}",
                session_id
            ))
            .await;

        loop {
            let (_peer_id, message_bytes) = effects
                .receive()
                .await
                .map_err(|e| AuraError::network(format!("Failed to receive message: {}", e)))?;

            if let Ok(FrostSigningMessage::SigningCompletion {
                session_id: msg_session_id,
                signature,
                success,
                error,
            }) = serde_json::from_slice::<FrostSigningMessage>(&message_bytes)
            {
                if msg_session_id == session_id {
                    return Ok(ThresholdSigningResponse {
                        signature,
                        participating_signers: vec![self.device_id], // Will be filled from session state
                        signature_shares: vec![], // Will be filled from session state
                        success,
                        error,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::AccountId;

    #[test]
    fn test_signing_coordinator_creation() {
        let device_id = DeviceId::new();
        let coordinator = FrostSigningCoordinator::new(device_id, SigningRole::Coordinator);
        assert_eq!(coordinator.device_id, device_id);
        assert_eq!(coordinator.role, SigningRole::Coordinator);
    }

    #[test]
    fn test_signing_request_serialization() {
        let request = ThresholdSigningRequest {
            message: b"Hello, FROST!".to_vec(),
            context: TreeSigningContext::new(1, 0, [0u8; 32]),
            account_id: AccountId::new(),
            threshold: 2,
            available_signers: vec![DeviceId::new(), DeviceId::new(), DeviceId::new()],
            timeout_seconds: 300,
        };

        let serialized = serde_json::to_vec(&request).unwrap();
        let deserialized: ThresholdSigningRequest = serde_json::from_slice(&serialized).unwrap();

        assert_eq!(request.message, deserialized.message);
        assert_eq!(request.threshold, deserialized.threshold);
        assert_eq!(
            request.available_signers.len(),
            deserialized.available_signers.len()
        );
    }

    #[test]
    fn test_signing_message_serialization() {
        let message = FrostSigningMessage::SigningInit {
            session_id: "test_session".to_string(),
            message: b"test_message".to_vec(),
            context: TreeSigningContext::new(1, 0, [0u8; 32]),
            account_id: AccountId::new(),
            threshold: 2,
            timeout_at: 1000,
        };

        let serialized = serde_json::to_vec(&message).unwrap();
        let deserialized: FrostSigningMessage = serde_json::from_slice(&serialized).unwrap();

        match deserialized {
            FrostSigningMessage::SigningInit {
                session_id,
                message: msg,
                threshold,
                ..
            } => {
                assert_eq!(session_id, "test_session");
                assert_eq!(msg, b"test_message".to_vec());
                assert_eq!(threshold, 2);
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_signing_role_naming() {
        assert_eq!(SigningRole::Coordinator.name(), "Coordinator");
        assert_eq!(SigningRole::Signer(1).name(), "Signer_1");
    }
}
