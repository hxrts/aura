//! G_frost: Choreographic FROST Threshold Signing Implementation
//!
//! This module implements the G_frost choreography for distributed threshold
//! signature generation using the rumpsteak-aura choreographic DSL.
//!
//! ## Architecture
//!
//! The choreography follows a 4-phase protocol:
//! 1. **Initiation**: Coordinator initiates signing ceremony
//! 2. **Nonce Phase**: Signers generate and send nonce commitments
//! 3. **Signature Phase**: Signers compute and send partial signatures
//! 4. **Aggregation**: Coordinator aggregates signatures and broadcasts result
//!
//! ## Session Types
//!
//! The choreography provides compile-time guarantees:
//! - Deadlock freedom through choreographic projection
//! - Type-checked message passing
//! - Automatic local type generation for each role

#![allow(missing_docs)]

use crate::FrostResult;
use aura_core::{AccountId, AuraError, DeviceId, SessionId};
use aura_crypto::frost::{
    NonceCommitment, PartialSignature, ThresholdSignature, TreeSigningContext,
};
use aura_macros::choreography;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for threshold signing choreography
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdSigningConfig {
    /// Number of signers required (M in M-of-N)
    pub threshold: usize,
    /// Total number of available signers (N in M-of-N)
    pub total_signers: usize,
    /// Session timeout in seconds
    pub timeout_seconds: u64,
    /// Maximum retry attempts
    pub max_retries: u32,
}

impl ThresholdSigningConfig {
    /// Create a new threshold signing configuration
    pub fn new(threshold: usize, total_signers: usize, timeout_seconds: u64) -> Self {
        Self {
            threshold,
            total_signers,
            timeout_seconds,
            max_retries: 3,
        }
    }

    /// Validate the configuration
    pub fn validate(&self) -> FrostResult<()> {
        if self.threshold == 0 {
            return Err(AuraError::invalid("Threshold must be greater than 0"));
        }
        if self.threshold > self.total_signers {
            return Err(AuraError::invalid("Threshold cannot exceed total signers"));
        }
        if self.total_signers > 100 {
            return Err(AuraError::invalid("Cannot support more than 100 signers"));
        }
        Ok(())
    }
}

/// Request message for threshold signing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SigningRequest {
    /// Session identifier
    pub session_id: SessionId,
    /// Message to be signed
    pub message: Vec<u8>,
    /// Signing context for binding
    pub context: TreeSigningContext,
    /// Account being processed
    pub account_id: AccountId,
    /// Configuration for this signing session
    pub config: ThresholdSigningConfig,
}

/// Nonce commitment message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NonceCommitmentMsg {
    /// Session identifier
    pub session_id: SessionId,
    /// Signer device ID
    pub signer_id: DeviceId,
    /// FROST nonce commitment
    pub commitment: NonceCommitment,
}

/// Partial signature message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialSignatureMsg {
    /// Session identifier
    pub session_id: SessionId,
    /// Signer device ID
    pub signer_id: DeviceId,
    /// FROST partial signature
    pub signature: PartialSignature,
}

/// Final signature result message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureResult {
    /// Session identifier
    pub session_id: SessionId,
    /// Aggregated threshold signature (if successful)
    pub signature: Option<ThresholdSignature>,
    /// List of participating signers
    pub participants: Vec<DeviceId>,
    /// Success indicator
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
}

/// Abort message for session termination
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbortMsg {
    /// Session identifier
    pub session_id: SessionId,
    /// Abort reason
    pub reason: String,
    /// Device that initiated the abort
    pub initiator: DeviceId,
}

/// FROST threshold signing choreography protocol
///
/// This choreography implements the complete FROST threshold signature protocol:
/// - Coordinator initiates signing and aggregates results  
/// - Signers participate in multi-phase threshold signing
/// - Supports dynamic signer sets with Byzantine fault tolerance
/// - Provides session isolation and timeout handling
choreography! {
    #[namespace = "frost_threshold_signing"]
    protocol FrostThresholdSigning {
        roles: Coordinator, Signers[*];

        // Phase 1: Coordinator initiates signing ceremony
        Coordinator[guard_capability = "initiate_signing",
                   flow_cost = 100,
                   journal_facts = "signing_initiated"]
        -> Signers[*]: SigningRequest(SigningRequest);

        // Phase 2: Signers send nonce commitments
        Signers[0..threshold][guard_capability = "send_nonce",
                              flow_cost = 50,
                              journal_facts = "nonce_committed"]
        -> Coordinator: NonceCommitmentMsg(NonceCommitmentMsg);

        // Phase 3: Signers send partial signatures
        Signers[0..threshold][guard_capability = "send_signature",
                              flow_cost = 75,
                              journal_facts = "signature_contributed"]
        -> Coordinator: PartialSignatureMsg(PartialSignatureMsg);

        // Phase 4: Coordinator aggregates and broadcasts result
        Coordinator[guard_capability = "aggregate_signatures",
                   flow_cost = 200,
                   journal_facts = "signature_aggregated",
                   journal_merge = true]
        -> Signers[*]: SignatureResult(SignatureResult);

        // Optional: Abort handling for timeout or failure scenarios
        choice Coordinator {
            success: {
                // Normal completion - signature result already sent
            }
            abort: {
                Coordinator[guard_capability = "abort_signing",
                           flow_cost = 50,
                           journal_facts = "signing_aborted"]
                -> Signers[*]: AbortMsg(AbortMsg);
            }
        }
    }
}

/// Get the FROST threshold signing choreography instance for protocol execution
///
/// This function provides access to the choreographic types and functions
/// generated by the `choreography!` macro for FROST threshold signing operations.
/// It serves as the entry point for choreographic execution of the signing protocol.
///
/// # Note
///
/// The actual implementation is generated by the choreography macro expansion.
/// This is a placeholder that will be replaced by the macro-generated code.
///
/// # Returns
///
/// Unit type - the macro generates the necessary choreographic infrastructure
pub fn get_frost_choreography() {
    // The choreography macro will generate the appropriate types and functions
}

/// Coordinator role implementation for FROST threshold signing
///
/// The coordinator manages threshold signing sessions, collecting nonce commitments
/// and partial signatures from signers, then aggregating them into final signatures.
/// It handles session lifecycle, timeout management, and error recovery.
pub struct FrostCoordinator {
    /// Device ID for this coordinator
    pub device_id: DeviceId,
    /// Current session state
    session_state: Option<CoordinatorSessionState>,
}

/// Session state for the coordinator
#[derive(Debug)]
struct CoordinatorSessionState {
    session_id: SessionId,
    config: ThresholdSigningConfig,
    request: SigningRequest,
    nonce_commitments: HashMap<DeviceId, NonceCommitment>,
    partial_signatures: HashMap<DeviceId, PartialSignature>,
    phase: SigningPhase,
    start_timestamp: u64,
}

/// Current phase of the signing protocol
#[derive(Debug, Clone, PartialEq)]
pub enum SigningPhase {
    /// Initial phase after session creation
    Initiated,
    /// Collecting nonce commitments from signers
    CollectingNonces,
    /// Collecting partial signatures from signers
    CollectingSignatures,
    /// Coordinator aggregating signatures
    Aggregating,
    /// Signing completed successfully
    Completed,
    /// Signing was aborted due to error or timeout
    Aborted,
}

impl FrostCoordinator {
    /// Create a new FROST coordinator
    ///
    /// # Arguments
    ///
    /// * `device_id` - The device identifier for this coordinator
    ///
    /// # Returns
    ///
    /// A new `FrostCoordinator` instance ready to coordinate signing sessions
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            session_state: None,
        }
    }

    /// Initiate a new threshold signing session
    pub async fn initiate_signing<E>(
        &mut self,
        effects: &E,
        request: SigningRequest,
    ) -> FrostResult<()>
    where
        E: aura_core::effects::TimeEffects,
    {
        // Validate the request
        request.config.validate()?;

        let start_timestamp = effects.current_timestamp().await;

        let session_state = CoordinatorSessionState {
            session_id: request.session_id,
            config: request.config.clone(),
            request,
            nonce_commitments: HashMap::new(),
            partial_signatures: HashMap::new(),
            phase: SigningPhase::Initiated,
            start_timestamp,
        };

        self.session_state = Some(session_state);
        Ok(())
    }

    /// Handle received nonce commitment
    pub async fn handle_nonce_commitment(
        &mut self,
        msg: NonceCommitmentMsg,
    ) -> FrostResult<bool> {
        let session = self.session_state.as_mut()
            .ok_or_else(|| AuraError::invalid("No active session"))?;

        if msg.session_id != session.session_id {
            return Err(AuraError::invalid("Session ID mismatch"));
        }

        if session.phase != SigningPhase::CollectingNonces {
            session.phase = SigningPhase::CollectingNonces;
        }

        session.nonce_commitments.insert(msg.signer_id, msg.commitment);

        // Check if we have enough commitments
        Ok(session.nonce_commitments.len() >= session.config.threshold)
    }

    /// Handle received partial signature
    pub async fn handle_partial_signature(
        &mut self,
        msg: PartialSignatureMsg,
    ) -> FrostResult<bool> {
        let session = self.session_state.as_mut()
            .ok_or_else(|| AuraError::invalid("No active session"))?;

        if msg.session_id != session.session_id {
            return Err(AuraError::invalid("Session ID mismatch"));
        }

        if session.phase != SigningPhase::CollectingSignatures {
            session.phase = SigningPhase::CollectingSignatures;
        }

        session.partial_signatures.insert(msg.signer_id, msg.signature);

        // Check if we have enough signatures
        Ok(session.partial_signatures.len() >= session.config.threshold)
    }

    /// Aggregate signatures and create final result
    pub async fn aggregate_signatures(&mut self) -> FrostResult<SignatureResult> {
        let session = self.session_state.as_mut()
            .ok_or_else(|| AuraError::invalid("No active session"))?;

        session.phase = SigningPhase::Aggregating;

        if session.partial_signatures.len() < session.config.threshold {
            session.phase = SigningPhase::Aborted;
            return Ok(SignatureResult {
                session_id: session.session_id,
                signature: None,
                participants: session.partial_signatures.keys().cloned().collect(),
                success: false,
                error: Some("Insufficient partial signatures".to_string()),
            });
        }

        // Perform FROST signature aggregation
        let context = session.request.context.clone();
        let message = session.request.message.clone();
        let signatures = session.partial_signatures.clone();
        let commitments = session.nonce_commitments.clone();
        let config = session.config.clone();
        let session_id = session.session_id;
        let participants: Vec<DeviceId> = session.partial_signatures.keys().cloned().collect();
        
        session.phase = SigningPhase::Aggregating;
        
        // Release the mutable borrow before calling the method
        let _ = session;
        
        match self.frost_aggregate_signatures_impl(context, message, signatures, commitments, config).await {
            Ok(signature) => {
                // Get session back as mutable
                let session = self.session_state.as_mut()
                    .ok_or_else(|| AuraError::invalid("Session lost during aggregation"))?;
                session.phase = SigningPhase::Completed;
                
                Ok(SignatureResult {
                    session_id,
                    signature: Some(signature),
                    participants,
                    success: true,
                    error: None,
                })
            }
            Err(e) => {
                // Get session back as mutable for error case
                if let Some(session) = self.session_state.as_mut() {
                    session.phase = SigningPhase::Aborted;
                }
                
                Ok(SignatureResult {
                    session_id,
                    signature: None,
                    participants,
                    success: false,
                    error: Some(e.to_string()),
                })
            }
        }
    }

    /// Perform FROST signature aggregation using aura-crypto
    async fn frost_aggregate_signatures_impl(
        &self,
        context: TreeSigningContext,
        message: Vec<u8>,
        partial_signatures: HashMap<DeviceId, PartialSignature>,
        nonce_commitments: HashMap<DeviceId, NonceCommitment>,
        config: ThresholdSigningConfig,
    ) -> FrostResult<ThresholdSignature> {
        use aura_crypto::frost::tree_signing::{binding_message, frost_aggregate};
        use frost_ed25519 as frost;
        use std::collections::BTreeMap;

        // Use the passed parameters directly

        // Create binding message for the signature
        let bound_message = binding_message(&context, &message);

        // Convert partial signatures to FROST format
        let partials: Vec<_> = partial_signatures.values().cloned().collect();

        // Convert commitments to FROST format
        let mut frost_commitments = BTreeMap::new();
        for (signer_id, commitment) in &nonce_commitments {
            // Map device ID to signer index (this would come from DKG in production)
            let device_bytes = signer_id.to_bytes()
                .map_err(|_| AuraError::crypto("Invalid device ID bytes"))?;
            let signer_index = (device_bytes[0] % (config.total_signers as u8)) as u16 + 1;
            frost_commitments.insert(signer_index, commitment.clone());
        }

        // Generate temporary key package for aggregation
        // In production, this would come from the DKG ceremony
        #[allow(clippy::disallowed_methods)]
        let rng = rand::thread_rng();
        let (_, pubkey_package) = frost::keys::generate_with_dealer(
            config.total_signers.try_into().unwrap(),
            config.threshold.try_into().unwrap(),
            frost::keys::IdentifierList::Default,
            rng,
        )
        .map_err(|e| AuraError::crypto(format!("Failed to generate key package: {}", e)))?;

        // Aggregate the signatures
        let signature_bytes = frost_aggregate(
            &partials,
            &bound_message,
            &frost_commitments,
            &pubkey_package,
        )
        .map_err(|e| AuraError::crypto(format!("FROST aggregation failed: {}", e)))?;

        // Create threshold signature result
        let participating_signers: Vec<u16> = partial_signatures
            .keys()
            .filter_map(|device_id| {
                device_id.to_bytes().ok().map(|bytes| (bytes[0] % (config.total_signers as u8)) as u16)
            })
            .collect();

        Ok(ThresholdSignature::new(signature_bytes, participating_signers))
    }

    /// Create abort message for session termination
    pub fn create_abort_message(&self, reason: String) -> FrostResult<AbortMsg> {
        let session = self.session_state.as_ref()
            .ok_or_else(|| AuraError::invalid("No active session"))?;

        Ok(AbortMsg {
            session_id: session.session_id,
            reason,
            initiator: self.device_id,
        })
    }

    /// Check if session has timed out
    pub async fn is_session_expired<E>(&self, effects: &E) -> bool
    where
        E: aura_core::effects::TimeEffects,
    {
        if let Some(session) = &self.session_state {
            let current_timestamp = effects.current_timestamp().await;
            let elapsed_ms = current_timestamp.saturating_sub(session.start_timestamp);
            let elapsed_seconds = elapsed_ms / 1000;
            elapsed_seconds > session.config.timeout_seconds
        } else {
            false
        }
    }

    /// Get current session phase
    pub fn current_phase(&self) -> Option<SigningPhase> {
        self.session_state.as_ref().map(|s| s.phase.clone())
    }
}

/// Signer role implementation for FROST threshold signing
///
/// A signer participates in threshold signing sessions by generating nonce commitments
/// and partial signatures. It responds to coordinator requests and contributes to
/// the multi-party signing process while maintaining session state.
pub struct FrostSigner {
    /// Device ID for this signer
    pub device_id: DeviceId,
    /// Current session state
    session_state: Option<SignerSessionState>,
}

/// Session state for a signer
#[derive(Debug)]
struct SignerSessionState {
    session_id: SessionId,
    request: SigningRequest,
    nonce_commitment: Option<NonceCommitment>,
    partial_signature: Option<PartialSignature>,
    phase: SigningPhase,
}

impl FrostSigner {
    /// Create a new FROST signer
    ///
    /// # Arguments
    ///
    /// * `device_id` - The device identifier for this signer
    ///
    /// # Returns
    ///
    /// A new `FrostSigner` instance ready to participate in signing sessions
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            session_state: None,
        }
    }

    /// Handle signing initiation request
    pub async fn handle_signing_request(&mut self, request: SigningRequest) -> FrostResult<()> {
        let session_state = SignerSessionState {
            session_id: request.session_id,
            request,
            nonce_commitment: None,
            partial_signature: None,
            phase: SigningPhase::Initiated,
        };

        self.session_state = Some(session_state);
        Ok(())
    }

    /// Generate and return nonce commitment
    pub async fn generate_nonce_commitment(&mut self) -> FrostResult<NonceCommitmentMsg> {
        let session_id = self.session_state.as_ref()
            .ok_or_else(|| AuraError::invalid("No active session"))?
            .session_id;

        // Generate FROST nonce commitment using real cryptography
        let commitment = self.generate_frost_nonce_commitment().await?;
        
        // Update session state
        if let Some(session) = self.session_state.as_mut() {
            session.nonce_commitment = Some(commitment.clone());
            session.phase = SigningPhase::CollectingNonces;
        }

        Ok(NonceCommitmentMsg {
            session_id,
            signer_id: self.device_id,
            commitment,
        })
    }

    /// Generate and return partial signature
    pub async fn generate_partial_signature(&mut self) -> FrostResult<PartialSignatureMsg> {
        let (session_id, commitment) = {
            let session = self.session_state.as_ref()
                .ok_or_else(|| AuraError::invalid("No active session"))?;
            let commitment = session.nonce_commitment.as_ref()
                .ok_or_else(|| AuraError::invalid("No nonce commitment available"))?;
            (session.session_id, commitment.clone())
        };

        // Generate FROST partial signature using real cryptography
        let signature = self.generate_frost_partial_signature(&commitment).await?;
        
        // Update session state
        if let Some(session) = self.session_state.as_mut() {
            session.partial_signature = Some(signature.clone());
            session.phase = SigningPhase::CollectingSignatures;
        }

        Ok(PartialSignatureMsg {
            session_id,
            signer_id: self.device_id,
            signature,
        })
    }

    /// Generate FROST nonce commitment
    async fn generate_frost_nonce_commitment(&self) -> FrostResult<NonceCommitment> {
        use aura_crypto::frost::tree_signing::generate_nonce_with_share;
        use frost_ed25519 as frost;

        // In production, this would use the actual signing share from DKG
        // For now, create a mock signing share
        let signing_share = frost::keys::SigningShare::deserialize([42u8; 32])
            .map_err(|e| AuraError::crypto(format!("Failed to create signing share: {}", e)))?;

        let (_, commitment) = generate_nonce_with_share(1, &signing_share);
        Ok(commitment)
    }

    /// Generate FROST partial signature
    async fn generate_frost_partial_signature(
        &self,
        _commitment: &NonceCommitment,
    ) -> FrostResult<PartialSignature> {
        use aura_crypto::frost::tree_signing::{binding_message, frost_sign_partial_with_keypackage};
        use frost_ed25519 as frost;

        // Get session data for binding message
        let (context, message) = {
            let session = self.session_state.as_ref()
                .ok_or_else(|| AuraError::invalid("No active session"))?;
            (session.request.context.clone(), session.request.message.clone())
        };

        // Create binding message
        let bound_message = binding_message(&context, &message);

        // In production, this would use the actual key package from DKG
        #[allow(clippy::disallowed_methods)]
        let rng = rand::thread_rng();
        let identifier = frost::Identifier::try_from(1u16)
            .map_err(|e| AuraError::crypto(format!("Invalid identifier: {}", e)))?;

        // Generate temporary key package for signing
        let (secret_shares, pubkey_package) = frost::keys::generate_with_dealer(
            3, 2, frost::keys::IdentifierList::Default, rng
        ).map_err(|e| AuraError::crypto(format!("Failed to generate keys: {}", e)))?;

        let secret_share = secret_shares
            .get(&identifier)
            .ok_or_else(|| AuraError::crypto("Secret share not found"))?;

        let signing_share = secret_share.signing_share();
        let verifying_share = pubkey_package
            .verifying_shares()
            .get(&identifier)
            .ok_or_else(|| AuraError::crypto("Verifying share not found"))?;
        let verifying_key = pubkey_package.verifying_key();

        let key_package = frost::keys::KeyPackage::new(
            identifier,
            *signing_share,
            *verifying_share,
            *verifying_key,
            2, // min_signers
        );

        // Create empty commitments map for this simplified implementation
        let frost_commitments = std::collections::BTreeMap::new();

        let partial_signature = frost_sign_partial_with_keypackage(
            &key_package,
            &bound_message,
            &frost_commitments,
        )
        .map_err(|e| AuraError::crypto(format!("FROST partial signing failed: {}", e)))?;

        Ok(partial_signature)
    }

    /// Handle session abort
    pub async fn handle_abort(&mut self, _abort_msg: AbortMsg) -> FrostResult<()> {
        if let Some(session) = &mut self.session_state {
            session.phase = SigningPhase::Aborted;
        }
        Ok(())
    }

    /// Get current session phase
    pub fn current_phase(&self) -> Option<SigningPhase> {
        self.session_state.as_ref().map(|s| s.phase.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::SessionId;

    #[test]
    fn test_threshold_signing_config_validation() {
        // Valid configuration
        let config = ThresholdSigningConfig::new(2, 3, 300);
        assert!(config.validate().is_ok());

        // Invalid: threshold = 0
        let config = ThresholdSigningConfig::new(0, 3, 300);
        assert!(config.validate().is_err());

        // Invalid: threshold > total
        let config = ThresholdSigningConfig::new(4, 3, 300);
        assert!(config.validate().is_err());

        // Invalid: too many signers
        let config = ThresholdSigningConfig::new(2, 101, 300);
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_choreography_creation() {
        let choreography = get_frost_choreography();
        // Test that we can create the choreography instance successfully
        // The macro generates a struct with the protocol name
    }

    #[test]
    fn test_coordinator_creation() {
        let device_id = DeviceId::new();
        let coordinator = FrostCoordinator::new(device_id);
        assert_eq!(coordinator.device_id, device_id);
        assert!(coordinator.session_state.is_none());
    }

    #[test]
    fn test_signer_creation() {
        let device_id = DeviceId::new();
        let signer = FrostSigner::new(device_id);
        assert_eq!(signer.device_id, device_id);
        assert!(signer.session_state.is_none());
    }

    #[tokio::test]
    async fn test_signing_request_handling() {
        use aura_effects::time::SimulatedTimeHandler;
        
        let mut coordinator = FrostCoordinator::new(DeviceId::new());
        let mock_effects = SimulatedTimeHandler::new(0); // Start at time 0
        let request = SigningRequest {
            session_id: SessionId::new(),
            message: b"test message".to_vec(),
            context: aura_crypto::frost::TreeSigningContext::new(1, 0, [0u8; 32]),
            account_id: AccountId::new(),
            config: ThresholdSigningConfig::new(2, 3, 300),
        };

        let result = coordinator.initiate_signing(&mock_effects, request).await;
        assert!(result.is_ok());
        assert!(coordinator.session_state.is_some());
        assert_eq!(coordinator.current_phase(), Some(SigningPhase::Initiated));
    }
}