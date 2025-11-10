//! G_frost: Main FROST Threshold Signing Choreography
//!
//! This module implements the G_frost choreography for distributed threshold
//! signature generation using the rumpsteak-aura choreographic programming framework.

use crate::{FrostError, FrostResult};
use aura_core::{AccountId, Cap, DeviceId};
use aura_crypto::frost::{
    Nonce, NonceCommitment, PartialSignature, Share, SigningSession, ThresholdSignature,
    TreeSigningContext,
};
use aura_mpst::{AuraRuntime, CapabilityGuard, JournalAnnotation, MpstError, MpstResult};
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
pub enum FrostMessage {
    /// Initiate signing ceremony
    SigningInit {
        /// Session ID for tracking
        session_id: String,
        /// Message to sign
        message: Vec<u8>,
        /// Signing context
        context: TreeSigningContext,
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

    /// Round 2: Partial signature submission
    PartialSignatureSubmission {
        /// Session ID
        session_id: String,
        /// Signer device ID
        signer_id: DeviceId,
        /// Partial signature
        partial_signature: PartialSignature,
    },

    /// Round 3: Signature aggregation result
    SignatureAggregation {
        /// Session ID
        session_id: String,
        /// Aggregated threshold signature
        threshold_signature: Option<ThresholdSignature>,
        /// Success status
        success: bool,
        /// Error if failed
        error: Option<String>,
    },

    /// Session abort notification
    SessionAbort {
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
pub enum FrostRole {
    /// Coordinator managing the signing process
    Coordinator,
    /// Signer participating in threshold signature
    Signer(u32),
    /// Aggregator collecting and combining signature shares
    Aggregator,
}

impl FrostRole {
    /// Get the name of this role
    pub fn name(&self) -> String {
        match self {
            FrostRole::Coordinator => "Coordinator".to_string(),
            FrostRole::Signer(id) => format!("Signer_{}", id),
            FrostRole::Aggregator => "Aggregator".to_string(),
        }
    }
}

/// G_frost choreography state
#[derive(Debug)]
pub struct FrostChoreographyState {
    /// Current signing request being processed
    current_request: Option<ThresholdSigningRequest>,
    /// Active signing sessions
    active_sessions: HashMap<String, SigningSession>,
    /// Nonce commitments by session ID and signer ID
    nonce_commitments: HashMap<String, HashMap<DeviceId, NonceCommitment>>,
    /// Partial signatures by session ID and signer ID
    partial_signatures: HashMap<String, HashMap<DeviceId, PartialSignature>>,
    /// Session timeouts by session ID
    session_timeouts: HashMap<String, u64>,
    /// Session progress tracking
    session_progress: HashMap<String, FrostSessionProgress>,
}

/// FROST session progress tracking
#[derive(Debug)]
pub struct FrostSessionProgress {
    /// Current round (0=init, 1=commitments, 2=signatures, 3=aggregation)
    current_round: usize,
    /// Participants committed to session
    participants: Vec<DeviceId>,
    /// Commitments received
    commitments_received: usize,
    /// Signatures received
    signatures_received: usize,
    /// Session start time
    started_at: u64,
}

impl FrostChoreographyState {
    /// Create new choreography state
    pub fn new() -> Self {
        Self {
            current_request: None,
            active_sessions: HashMap::new(),
            nonce_commitments: HashMap::new(),
            partial_signatures: HashMap::new(),
            session_timeouts: HashMap::new(),
            session_progress: HashMap::new(),
        }
    }

    /// Initialize a new signing session
    pub fn init_signing_session(
        &mut self,
        session_id: String,
        request: ThresholdSigningRequest,
    ) -> Result<(), FrostError> {
        // Validate threshold configuration
        if request.threshold == 0 || request.threshold > request.available_signers.len() {
            return Err(FrostError::invalid(format!(
                "Invalid threshold: {} of {} signers",
                request.threshold,
                request.available_signers.len()
            )));
        }

        // Create signing session
        let session = SigningSession::new(
            session_id.clone(),
            request.message.clone(),
            request.context.clone(),
            request.threshold as u16,
            request
                .available_signers
                .iter()
                .enumerate()
                .map(|(i, _)| i as u16)
                .collect(),
        );

        // Initialize progress tracking
        let progress = FrostSessionProgress {
            current_round: 0,
            participants: request.available_signers.clone(),
            commitments_received: 0,
            signatures_received: 0,
            started_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        // Calculate timeout
        let timeout_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + request.timeout_seconds;

        self.active_sessions.insert(session_id.clone(), session);
        self.session_progress.insert(session_id.clone(), progress);
        self.session_timeouts.insert(session_id.clone(), timeout_at);
        self.nonce_commitments
            .insert(session_id.clone(), HashMap::new());
        self.partial_signatures
            .insert(session_id.clone(), HashMap::new());

        Ok(())
    }

    /// Add nonce commitment for a signer
    pub fn add_nonce_commitment(
        &mut self,
        session_id: &str,
        signer_id: DeviceId,
        commitment: NonceCommitment,
    ) -> Result<(), FrostError> {
        if let Some(commitments) = self.nonce_commitments.get_mut(session_id) {
            commitments.insert(signer_id, commitment);

            // Update progress
            if let Some(progress) = self.session_progress.get_mut(session_id) {
                progress.commitments_received = commitments.len();
                if progress.current_round == 0 {
                    progress.current_round = 1;
                }
            }

            Ok(())
        } else {
            Err(FrostError::not_found(format!(
                "Session not found: {}",
                session_id
            )))
        }
    }

    /// Add partial signature for a signer
    pub fn add_partial_signature(
        &mut self,
        session_id: &str,
        signer_id: DeviceId,
        signature: PartialSignature,
    ) -> Result<(), FrostError> {
        if let Some(signatures) = self.partial_signatures.get_mut(session_id) {
            signatures.insert(signer_id, signature);

            // Update progress
            if let Some(progress) = self.session_progress.get_mut(session_id) {
                progress.signatures_received = signatures.len();
                if progress.current_round == 1 {
                    progress.current_round = 2;
                }
            }

            Ok(())
        } else {
            Err(FrostError::not_found(format!(
                "Session not found: {}",
                session_id
            )))
        }
    }

    /// Check if session has enough commitments for threshold
    pub fn has_threshold_commitments(&self, session_id: &str) -> bool {
        if let (Some(commitments), Some(session)) = (
            self.nonce_commitments.get(session_id),
            self.active_sessions.get(session_id),
        ) {
            commitments.len() >= session.threshold() as usize
        } else {
            false
        }
    }

    /// Check if session has enough signatures for threshold
    pub fn has_threshold_signatures(&self, session_id: &str) -> bool {
        if let (Some(signatures), Some(session)) = (
            self.partial_signatures.get(session_id),
            self.active_sessions.get(session_id),
        ) {
            signatures.len() >= session.threshold() as usize
        } else {
            false
        }
    }

    /// Check if session has timed out
    pub fn is_session_timed_out(&self, session_id: &str) -> bool {
        if let Some(&timeout_at) = self.session_timeouts.get(session_id) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            now > timeout_at
        } else {
            false
        }
    }

    /// Get partial signatures for aggregation
    pub fn get_partial_signatures(&self, session_id: &str) -> Vec<PartialSignature> {
        self.partial_signatures
            .get(session_id)
            .map(|signatures| signatures.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Complete session and clean up
    pub fn complete_session(&mut self, session_id: &str) {
        self.active_sessions.remove(session_id);
        self.nonce_commitments.remove(session_id);
        self.partial_signatures.remove(session_id);
        self.session_timeouts.remove(session_id);
        self.session_progress.remove(session_id);
    }
}

/// G_frost choreography implementation
///
/// This choreography coordinates distributed threshold signing with:
/// - Capability guards for authorization: `[guard: threshold_sign ≤ caps]`
/// - Journal coupling for CRDT integration: `[▷ Δthreshold_sig]`
/// - Leakage tracking for privacy: `[leak: signing_metadata]`
#[derive(Debug)]
pub struct FrostChoreography {
    /// Local device role
    role: FrostRole,
    /// Choreography state
    state: Mutex<FrostChoreographyState>,
    /// MPST runtime
    runtime: AuraRuntime,
}

impl FrostChoreography {
    /// Create a new G_frost choreography
    pub fn new(role: FrostRole, runtime: AuraRuntime) -> Self {
        Self {
            role,
            state: Mutex::new(FrostChoreographyState::new()),
            runtime,
        }
    }

    /// Execute the choreography
    pub async fn execute(
        &self,
        request: ThresholdSigningRequest,
    ) -> FrostResult<ThresholdSigningResponse> {
        let mut state = self.state.lock().await;
        state.current_request = Some(request.clone());
        let session_id = uuid::Uuid::new_v4().to_string();
        state.init_signing_session(session_id.clone(), request.clone())?;
        drop(state);

        match self.role {
            FrostRole::Coordinator => self.execute_coordinator(request, session_id).await,
            FrostRole::Signer(_) => self.execute_signer(session_id).await,
            FrostRole::Aggregator => self.execute_aggregator(session_id).await,
        }
    }

    /// Execute as coordinator
    async fn execute_coordinator(
        &self,
        request: ThresholdSigningRequest,
        session_id: String,
    ) -> FrostResult<ThresholdSigningResponse> {
        tracing::info!(
            "Executing G_frost as coordinator for account: {}",
            request.account_id
        );

        // Apply capability guard: [guard: threshold_sign ≤ caps]
        let signing_cap = Cap::new(); // TODO: Create proper threshold signing capability
        let guard = CapabilityGuard::new(signing_cap);
        guard.enforce(self.runtime.capabilities()).map_err(|_| {
            FrostError::permission_denied(
                "Insufficient capabilities for threshold signing".to_string(),
            )
        })?;

        // Validate request
        if request.available_signers.len() < request.threshold {
            return Err(FrostError::invalid(format!(
                "Insufficient signers: need {} but have {}",
                request.threshold,
                request.available_signers.len()
            )));
        }

        tracing::info!(
            "Initiating FROST signing ceremony: {} of {} signers required",
            request.threshold,
            request.available_signers.len()
        );

        // Send signing init to all available signers
        // TODO: Implement actual message sending

        // Wait for nonce commitments
        // TODO: Implement commitment collection

        // Coordinate signature collection
        // TODO: Implement signature coordination

        // Apply journal annotation: [▷ Δthreshold_sig]
        let journal_annotation =
            JournalAnnotation::add_facts("FROST threshold signing ceremony".to_string());
        tracing::info!("Applied journal annotation: {:?}", journal_annotation);

        // TODO fix - For now, return a placeholder response
        Ok(ThresholdSigningResponse {
            signature: None,
            participating_signers: request.available_signers,
            signature_shares: Vec::new(),
            success: false,
            error: Some("G_frost choreography execution not fully implemented".to_string()),
        })
    }

    /// Execute as signer
    async fn execute_signer(&self, session_id: String) -> FrostResult<ThresholdSigningResponse> {
        tracing::info!("Executing G_frost as signer for session: {}", session_id);

        // Wait for signing init
        // TODO: Implement message receiving

        // Generate and send nonce commitment
        // TODO: Implement nonce generation and commitment

        // Wait for other commitments and generate partial signature
        // TODO: Implement partial signature generation

        // Send partial signature
        // TODO: Implement signature submission

        Ok(ThresholdSigningResponse {
            signature: None,
            participating_signers: Vec::new(),
            signature_shares: Vec::new(),
            success: false,
            error: Some("G_frost choreography execution not fully implemented".to_string()),
        })
    }

    /// Execute as aggregator
    async fn execute_aggregator(
        &self,
        session_id: String,
    ) -> FrostResult<ThresholdSigningResponse> {
        tracing::info!(
            "Executing G_frost as aggregator for session: {}",
            session_id
        );

        // Wait for partial signatures
        // TODO: Implement signature collection

        // Aggregate signatures into threshold signature
        // TODO: Implement signature aggregation

        // Verify aggregated signature
        // TODO: Implement signature verification

        // Broadcast result to all participants
        // TODO: Implement result broadcasting

        Ok(ThresholdSigningResponse {
            signature: None,
            participating_signers: Vec::new(),
            signature_shares: Vec::new(),
            success: false,
            error: Some("G_frost choreography execution not fully implemented".to_string()),
        })
    }
}

/// FROST threshold signing coordinator
#[derive(Debug)]
pub struct FrostSigningCoordinator {
    /// Local runtime
    runtime: AuraRuntime,
    /// Current choreography
    choreography: Option<FrostChoreography>,
}

impl FrostSigningCoordinator {
    /// Create a new FROST signing coordinator
    pub fn new(runtime: AuraRuntime) -> Self {
        Self {
            runtime,
            choreography: None,
        }
    }

    /// Execute threshold signing using the G_frost choreography
    pub async fn execute_threshold_signing(
        &mut self,
        request: ThresholdSigningRequest,
    ) -> FrostResult<ThresholdSigningResponse> {
        tracing::info!(
            "Starting threshold signing for account: {}",
            request.account_id
        );

        // Validate request
        if request.threshold == 0 {
            return Err(FrostError::invalid(format!(
                "Invalid threshold: 0 of {} signers",
                request.available_signers.len()
            )));
        }

        if request.available_signers.is_empty() {
            return Err(FrostError::invalid(format!(
                "Insufficient signers: need {} but have 0",
                request.threshold
            )));
        }

        // Create choreography with coordinator role
        let choreography = FrostChoreography::new(FrostRole::Coordinator, self.runtime.clone());

        // Execute the choreography
        let result = choreography.execute(request).await;

        // Store choreography for potential follow-up operations
        self.choreography = Some(choreography);

        result
    }

    /// Get the current runtime
    pub fn runtime(&self) -> &AuraRuntime {
        &self.runtime
    }

    /// Check if a choreography is currently active
    pub fn has_active_choreography(&self) -> bool {
        self.choreography.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{AccountId, Cap, DeviceId, Journal};
    use aura_crypto::frost::TreeSigningContext;

    #[tokio::test]
    async fn test_choreography_state_creation() {
        let mut state = FrostChoreographyState::new();

        let session_id = "test_session".to_string();
        let request = ThresholdSigningRequest {
            message: b"test message".to_vec(),
            context: TreeSigningContext::new(b"test context"),
            account_id: AccountId::new(),
            threshold: 2,
            available_signers: vec![DeviceId::new(), DeviceId::new(), DeviceId::new()],
            timeout_seconds: 300,
        };

        assert!(state
            .init_signing_session(session_id.clone(), request)
            .is_ok());
        assert!(!state.has_threshold_commitments(&session_id));
        assert!(!state.has_threshold_signatures(&session_id));
        assert!(!state.is_session_timed_out(&session_id));
    }

    #[tokio::test]
    async fn test_choreography_creation() {
        let device_id = DeviceId::new();
        let runtime = AuraRuntime::new(device_id, Cap::top(), Journal::new());

        let choreography = FrostChoreography::new(FrostRole::Coordinator, runtime);

        assert_eq!(choreography.role, FrostRole::Coordinator);
    }

    #[tokio::test]
    async fn test_frost_coordinator() {
        let device_id = DeviceId::new();
        let runtime = AuraRuntime::new(device_id, Cap::top(), Journal::new());

        let mut coordinator = FrostSigningCoordinator::new(runtime);
        assert!(!coordinator.has_active_choreography());

        let request = ThresholdSigningRequest {
            message: b"test message".to_vec(),
            context: TreeSigningContext::new(b"test context"),
            account_id: AccountId::new(),
            threshold: 2,
            available_signers: vec![DeviceId::new(), DeviceId::new()],
            timeout_seconds: 300,
        };

        // Note: This will return an error since choreography is not fully implemented
        let result = coordinator.execute_threshold_signing(request).await;
        assert!(result.is_err());
        assert!(coordinator.has_active_choreography());
    }

    #[tokio::test]
    async fn test_invalid() {
        let mut state = FrostChoreographyState::new();

        let session_id = "test_session".to_string();
        let request = ThresholdSigningRequest {
            message: b"test message".to_vec(),
            context: TreeSigningContext::new(b"test context"),
            account_id: AccountId::new(),
            threshold: 5, // More than available signers
            available_signers: vec![DeviceId::new(), DeviceId::new()], // Only 2 signers
            timeout_seconds: 300,
        };

        let result = state.init_signing_session(session_id, request);
        assert!(result.is_err()); // Error should be returned for invalid threshold
    }
}
