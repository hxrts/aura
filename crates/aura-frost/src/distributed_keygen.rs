//! G_dkg: Distributed Key Generation Choreography
//!
//! This module implements the G_dkg choreography for distributed threshold
//! key generation using the rumpsteak-aura choreographic programming framework.

use crate::{FrostError, FrostResult};
use aura_core::{AccountId, Cap, DeviceId};
use aura_crypto::frost::{PublicKeyPackage, Share};
use aura_mpst::{AuraRuntime, CapabilityGuard, JournalAnnotation, MpstError, MpstResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::Mutex;

/// Distributed key generation request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkgRequest {
    /// Account for key generation
    pub account_id: AccountId,
    /// Required threshold (M in M-of-N)
    pub threshold: usize,
    /// Total number of participants
    pub total_participants: usize,
    /// Participating devices
    pub participants: Vec<DeviceId>,
    /// Session timeout in seconds
    pub timeout_seconds: u64,
}

/// Distributed key generation response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkgResponse {
    /// Generated public key package
    pub public_key_package: Option<PublicKeyPackage>,
    /// Participating devices
    pub participants: Vec<DeviceId>,
    /// Key generation successful
    pub success: bool,
    /// Error message if any
    pub error: Option<String>,
}

/// Message types for the G_dkg choreography
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DkgMessage {
    /// Initiate DKG ceremony
    DkgInit {
        /// Session ID for tracking
        session_id: String,
        /// Account context
        account_id: AccountId,
        /// Threshold configuration
        threshold: usize,
        /// Total participants
        total_participants: usize,
        /// Session timeout
        timeout_at: u64,
    },

    /// Round 1: Share commitment
    ShareCommitment {
        /// Session ID
        session_id: String,
        /// Participant device ID
        participant_id: DeviceId,
        /// Share commitment
        commitment: Vec<u8>, // Serialized commitment
    },

    /// Round 2: Share revelation
    ShareRevelation {
        /// Session ID
        session_id: String,
        /// Participant device ID
        participant_id: DeviceId,
        /// Revealed share
        share: Vec<u8>, // Serialized share
    },

    /// Round 3: Verification results
    VerificationResult {
        /// Session ID
        session_id: String,
        /// Participant device ID
        participant_id: DeviceId,
        /// Verification success
        verified: bool,
        /// Complaints if any
        complaints: Vec<DeviceId>,
    },

    /// Round 4: DKG completion
    DkgCompletion {
        /// Session ID
        session_id: String,
        /// Generated public key package
        public_key_package: Option<PublicKeyPackage>,
        /// Success status
        success: bool,
        /// Error if failed
        error: Option<String>,
    },

    /// Session abort notification
    DkgAbort {
        /// Session ID
        session_id: String,
        /// Reason for abort
        reason: String,
        /// Device initiating abort
        initiator: DeviceId,
    },
}

/// Roles in the G_dkg choreography
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DkgRole {
    /// Coordinator managing the DKG process
    Coordinator,
    /// Participant contributing to key generation
    Participant(u32),
    /// Dealer distributing initial shares (for fallback)
    Dealer,
}

impl DkgRole {
    /// Get the name of this role
    pub fn name(&self) -> String {
        match self {
            DkgRole::Coordinator => "Coordinator".to_string(),
            DkgRole::Participant(id) => format!("Participant_{}", id),
            DkgRole::Dealer => "Dealer".to_string(),
        }
    }
}

/// G_dkg choreography state
#[derive(Debug)]
pub struct DkgChoreographyState {
    /// Current DKG request being processed
    current_request: Option<DkgRequest>,
    /// Share commitments by session ID and participant ID
    share_commitments: HashMap<String, HashMap<DeviceId, Vec<u8>>>,
    /// Revealed shares by session ID and participant ID
    revealed_shares: HashMap<String, HashMap<DeviceId, Vec<u8>>>,
    /// Verification results by session ID and participant ID
    verification_results: HashMap<String, HashMap<DeviceId, (bool, Vec<DeviceId>)>>,
    /// Session timeouts by session ID
    session_timeouts: HashMap<String, u64>,
    /// Session progress tracking
    session_progress: HashMap<String, DkgSessionProgress>,
}

/// DKG session progress tracking
#[derive(Debug)]
pub struct DkgSessionProgress {
    /// Current round (0=init, 1=commitments, 2=revelations, 3=verification, 4=completion)
    current_round: usize,
    /// Participants in session
    participants: Vec<DeviceId>,
    /// Commitments received
    commitments_received: usize,
    /// Revelations received
    revelations_received: usize,
    /// Verifications received
    verifications_received: usize,
    /// Session start time
    started_at: u64,
}

impl DkgChoreographyState {
    /// Create new choreography state
    pub fn new() -> Self {
        Self {
            current_request: None,
            share_commitments: HashMap::new(),
            revealed_shares: HashMap::new(),
            verification_results: HashMap::new(),
            session_timeouts: HashMap::new(),
            session_progress: HashMap::new(),
        }
    }

    /// Initialize a new DKG session
    pub fn init_dkg_session(
        &mut self,
        session_id: String,
        request: DkgRequest,
    ) -> Result<(), FrostError> {
        // Validate threshold configuration
        if request.threshold == 0 || request.threshold > request.total_participants {
            return Err(FrostError::invalid(format!(
                "Invalid threshold: {} of {} participants",
                request.threshold, request.total_participants
            )));
        }

        if request.participants.len() != request.total_participants {
            return Err(FrostError::crypto(format!(
                "Participant count mismatch: expected {}, got {}",
                request.total_participants,
                request.participants.len()
            )));
        }

        // Initialize progress tracking
        let progress = DkgSessionProgress {
            current_round: 0,
            participants: request.participants.clone(),
            commitments_received: 0,
            revelations_received: 0,
            verifications_received: 0,
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

        self.session_progress.insert(session_id.clone(), progress);
        self.session_timeouts.insert(session_id.clone(), timeout_at);
        self.share_commitments
            .insert(session_id.clone(), HashMap::new());
        self.revealed_shares
            .insert(session_id.clone(), HashMap::new());
        self.verification_results
            .insert(session_id.clone(), HashMap::new());

        Ok(())
    }

    /// Add share commitment for a participant
    pub fn add_share_commitment(
        &mut self,
        session_id: &str,
        participant_id: DeviceId,
        commitment: Vec<u8>,
    ) -> Result<(), FrostError> {
        if let Some(commitments) = self.share_commitments.get_mut(session_id) {
            commitments.insert(participant_id, commitment);

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
                "DKG session not found: {}",
                session_id
            )))
        }
    }

    /// Add revealed share for a participant
    pub fn add_revealed_share(
        &mut self,
        session_id: &str,
        participant_id: DeviceId,
        share: Vec<u8>,
    ) -> Result<(), FrostError> {
        if let Some(shares) = self.revealed_shares.get_mut(session_id) {
            shares.insert(participant_id, share);

            // Update progress
            if let Some(progress) = self.session_progress.get_mut(session_id) {
                progress.revelations_received = shares.len();
                if progress.current_round == 1 {
                    progress.current_round = 2;
                }
            }

            Ok(())
        } else {
            Err(FrostError::not_found(format!(
                "DKG session not found: {}",
                session_id
            )))
        }
    }

    /// Add verification result for a participant
    pub fn add_verification_result(
        &mut self,
        session_id: &str,
        participant_id: DeviceId,
        verified: bool,
        complaints: Vec<DeviceId>,
    ) -> Result<(), FrostError> {
        if let Some(results) = self.verification_results.get_mut(session_id) {
            results.insert(participant_id, (verified, complaints));

            // Update progress
            if let Some(progress) = self.session_progress.get_mut(session_id) {
                progress.verifications_received = results.len();
                if progress.current_round == 2 {
                    progress.current_round = 3;
                }
            }

            Ok(())
        } else {
            Err(FrostError::not_found(format!(
                "DKG session not found: {}",
                session_id
            )))
        }
    }

    /// Check if all participants have submitted commitments
    pub fn has_all_commitments(&self, session_id: &str) -> bool {
        if let (Some(commitments), Some(progress)) = (
            self.share_commitments.get(session_id),
            self.session_progress.get(session_id),
        ) {
            commitments.len() == progress.participants.len()
        } else {
            false
        }
    }

    /// Check if all participants have revealed shares
    pub fn has_all_revelations(&self, session_id: &str) -> bool {
        if let (Some(shares), Some(progress)) = (
            self.revealed_shares.get(session_id),
            self.session_progress.get(session_id),
        ) {
            shares.len() == progress.participants.len()
        } else {
            false
        }
    }

    /// Check if all participants have submitted verification results
    pub fn has_all_verifications(&self, session_id: &str) -> bool {
        if let (Some(results), Some(progress)) = (
            self.verification_results.get(session_id),
            self.session_progress.get(session_id),
        ) {
            results.len() == progress.participants.len()
        } else {
            false
        }
    }

    /// Check if DKG verification was successful (no complaints)
    pub fn is_dkg_successful(&self, session_id: &str) -> bool {
        if let Some(results) = self.verification_results.get(session_id) {
            results
                .values()
                .all(|(verified, complaints)| *verified && complaints.is_empty())
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

    /// Complete session and clean up
    pub fn complete_session(&mut self, session_id: &str) {
        self.share_commitments.remove(session_id);
        self.revealed_shares.remove(session_id);
        self.verification_results.remove(session_id);
        self.session_timeouts.remove(session_id);
        self.session_progress.remove(session_id);
    }
}

/// G_dkg choreography implementation
///
/// This choreography coordinates distributed key generation with:
/// - Capability guards for authorization: `[guard: key_generate ≤ caps]`
/// - Journal coupling for CRDT integration: `[▷ Δkey_generation]`
/// - Leakage tracking for privacy: `[leak: keygen_metadata]`
#[derive(Debug)]
pub struct DkgChoreography {
    /// Local device role
    role: DkgRole,
    /// Choreography state
    state: Mutex<DkgChoreographyState>,
    /// MPST runtime
    runtime: AuraRuntime,
}

impl DkgChoreography {
    /// Create a new G_dkg choreography
    pub fn new(role: DkgRole, runtime: AuraRuntime) -> Self {
        Self {
            role,
            state: Mutex::new(DkgChoreographyState::new()),
            runtime,
        }
    }

    /// Execute the choreography
    pub async fn execute(&self, request: DkgRequest) -> FrostResult<DkgResponse> {
        let mut state = self.state.lock().await;
        state.current_request = Some(request.clone());
        let session_id = uuid::Uuid::new_v4().to_string();
        state.init_dkg_session(session_id.clone(), request.clone())?;
        drop(state);

        match self.role {
            DkgRole::Coordinator => self.execute_coordinator(request, session_id).await,
            DkgRole::Participant(_) => self.execute_participant(session_id).await,
            DkgRole::Dealer => self.execute_dealer(session_id).await,
        }
    }

    /// Execute as coordinator
    async fn execute_coordinator(
        &self,
        request: DkgRequest,
        session_id: String,
    ) -> FrostResult<DkgResponse> {
        tracing::info!(
            "Executing G_dkg as coordinator for account: {}",
            request.account_id
        );

        // Apply capability guard: [guard: key_generate ≤ caps]
        let keygen_cap = Cap::new(); // TODO: Create proper key generation capability
        let guard = CapabilityGuard::new(keygen_cap);
        guard.enforce(self.runtime.capabilities()).map_err(|_| {
            FrostError::permission_denied(
                "Insufficient capabilities for key generation".to_string(),
            )
        })?;

        // Validate request
        if request.participants.len() < request.threshold {
            return Err(FrostError::invalid(format!(
                "Insufficient signers: need {} but have {}",
                request.threshold,
                request.participants.len()
            )));
        }

        tracing::info!(
            "Initiating DKG ceremony: {} of {} threshold",
            request.threshold,
            request.total_participants
        );

        // Send DKG init to all participants
        // TODO: Implement actual message sending

        // Coordinate share commitment round
        // TODO: Implement commitment coordination

        // Coordinate share revelation round
        // TODO: Implement revelation coordination

        // Coordinate verification round
        // TODO: Implement verification coordination

        // Apply journal annotation: [▷ Δkey_generation]
        let journal_annotation =
            JournalAnnotation::add_facts("DKG key generation ceremony".to_string());
        tracing::info!("Applied journal annotation: {:?}", journal_annotation);

        // TODO fix - For now, return a placeholder response
        Ok(DkgResponse {
            public_key_package: None,
            participants: request.participants,
            success: false,
            error: Some("G_dkg choreography execution not fully implemented".to_string()),
        })
    }

    /// Execute as participant
    async fn execute_participant(&self, session_id: String) -> FrostResult<DkgResponse> {
        tracing::info!("Executing G_dkg as participant for session: {}", session_id);

        // Wait for DKG init
        // TODO: Implement message receiving

        // Generate and send share commitment
        // TODO: Implement commitment generation

        // Wait for all commitments and reveal share
        // TODO: Implement share revelation

        // Verify all revealed shares
        // TODO: Implement share verification

        // Send verification result
        // TODO: Implement verification submission

        Ok(DkgResponse {
            public_key_package: None,
            participants: Vec::new(),
            success: false,
            error: Some("G_dkg choreography execution not fully implemented".to_string()),
        })
    }

    /// Execute as dealer (fallback trusted setup)
    async fn execute_dealer(&self, session_id: String) -> FrostResult<DkgResponse> {
        tracing::info!("Executing G_dkg as dealer for session: {}", session_id);

        // Generate shares using trusted dealer
        // TODO: Implement trusted dealer key generation

        // Distribute shares to participants
        // TODO: Implement share distribution

        // Coordinate verification
        // TODO: Implement dealer verification

        Ok(DkgResponse {
            public_key_package: None,
            participants: Vec::new(),
            success: false,
            error: Some("G_dkg choreography execution not fully implemented".to_string()),
        })
    }
}

/// DKG coordinator
#[derive(Debug)]
pub struct DkgCoordinator {
    /// Local runtime
    runtime: AuraRuntime,
    /// Current choreography
    choreography: Option<DkgChoreography>,
}

impl DkgCoordinator {
    /// Create a new DKG coordinator
    pub fn new(runtime: AuraRuntime) -> Self {
        Self {
            runtime,
            choreography: None,
        }
    }

    /// Execute distributed key generation using the G_dkg choreography
    pub async fn execute_dkg(&mut self, request: DkgRequest) -> FrostResult<DkgResponse> {
        tracing::info!("Starting DKG for account: {}", request.account_id);

        // Validate request
        if request.threshold == 0 {
            return Err(FrostError::invalid(format!(
                "Invalid threshold: 0 of {} participants",
                request.total_participants
            )));
        }

        if request.participants.is_empty() {
            return Err(FrostError::invalid(format!(
                "Insufficient signers: need {} but have 0",
                request.threshold
            )));
        }

        // Create choreography with coordinator role
        let choreography = DkgChoreography::new(DkgRole::Coordinator, self.runtime.clone());

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

    #[tokio::test]
    async fn test_dkg_state_creation() {
        let mut state = DkgChoreographyState::new();

        let session_id = "test_dkg_session".to_string();
        let request = DkgRequest {
            account_id: AccountId::new(),
            threshold: 2,
            total_participants: 3,
            participants: vec![DeviceId::new(), DeviceId::new(), DeviceId::new()],
            timeout_seconds: 600,
        };

        assert!(state.init_dkg_session(session_id.clone(), request).is_ok());
        assert!(!state.has_all_commitments(&session_id));
        assert!(!state.has_all_revelations(&session_id));
        assert!(!state.has_all_verifications(&session_id));
        assert!(!state.is_session_timed_out(&session_id));
    }

    #[tokio::test]
    async fn test_dkg_coordinator() {
        let device_id = DeviceId::new();
        let runtime = AuraRuntime::new(device_id, Cap::top(), Journal::new());

        let mut coordinator = DkgCoordinator::new(runtime);
        assert!(!coordinator.has_active_choreography());

        let request = DkgRequest {
            account_id: AccountId::new(),
            threshold: 2,
            total_participants: 3,
            participants: vec![DeviceId::new(), DeviceId::new(), DeviceId::new()],
            timeout_seconds: 600,
        };

        // Note: This will return an error since choreography is not fully implemented
        let result = coordinator.execute_dkg(request).await;
        assert!(result.is_err());
        assert!(coordinator.has_active_choreography());
    }

    #[tokio::test]
    async fn test_invalid_dkg_threshold() {
        let mut state = DkgChoreographyState::new();

        let session_id = "test_session".to_string();
        let request = DkgRequest {
            account_id: AccountId::new(),
            threshold: 5, // More than total participants
            total_participants: 3,
            participants: vec![DeviceId::new(), DeviceId::new(), DeviceId::new()],
            timeout_seconds: 600,
        };

        let result = state.init_dkg_session(session_id, request);
        assert!(result.is_err()); // Error should be returned for invalid threshold
    }
}
