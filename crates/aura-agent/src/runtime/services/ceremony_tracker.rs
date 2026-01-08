//! # Guardian Ceremony Tracker
//!
//! Tracks state of in-progress guardian ceremonies across the agent runtime.
//!
//! ## Responsibilities
//!
//! - Register new ceremonies with threshold configuration
//! - Track which guardians have accepted invitations
//! - Determine when threshold is reached for ceremony completion
//! - Provide ceremony status for monitoring
//! - Handle ceremony failures and timeouts
//!
//! ## Architecture
//!
//! The tracker maintains in-memory state of active ceremonies. When guardians
//! accept invitations (via `GuardianBinding` facts in the journal), the ceremony
//! state is updated. Once threshold is reached, the ceremony is marked complete
//! and `commit_guardian_key_rotation()` is triggered.
//!
//! ## Status Types
//!
//! This module uses `TrackedCeremony` for internal runtime state tracking, which
//! can be converted to `CeremonyStatus` (from `aura-core::domain::status`) for
//! UI display and consistency tracking.

use super::state::with_state_mut_validated;
use aura_app::core::IntentError;
use aura_app::runtime_bridge::CeremonyKind;
use aura_core::ceremony::{SupersessionReason, SupersessionRecord};
use aura_core::domain::status::{
    CeremonyResponse, CeremonyState as StatusCeremonyState, CeremonyStatus, ParticipantResponse,
    SupersessionReason as StatusSupersessionReason,
};
use aura_core::identifiers::{AuthorityId, CeremonyId};
use aura_core::query::ConsensusId;
use aura_core::threshold::{policy_for, AgreementMode, CeremonyFlow, ParticipantIdentity};
use aura_core::time::PhysicalTime;
use aura_core::{DeviceId, Hash32};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{Duration, Instant};

/// Tracks state of guardian ceremonies
#[derive(Clone)]
pub struct CeremonyTracker {
    state: Arc<RwLock<CeremonyTrackerState>>,
}

#[derive(Debug, Default)]
struct CeremonyTrackerState {
    ceremonies: HashMap<CeremonyId, TrackedCeremony>,
    /// Supersession records for audit trail
    supersession_records: Vec<SupersessionRecord>,
}

impl CeremonyTrackerState {
    fn validate(&self) -> Result<(), String> {
        for (ceremony_id, state) in &self.ceremonies {
            if state.threshold_k == 0 {
                return Err(format!("ceremony {} has zero threshold", ceremony_id));
            }
            if state.threshold_k > state.total_n {
                return Err(format!(
                    "ceremony {} threshold {} exceeds total {}",
                    ceremony_id, state.threshold_k, state.total_n
                ));
            }
            if state.total_n as usize != state.participants.len() {
                return Err(format!(
                    "ceremony {} total_n {} does not match participant count {}",
                    ceremony_id,
                    state.total_n,
                    state.participants.len()
                ));
            }
            if !state.accepted_participants.is_subset(&state.participants) {
                return Err(format!(
                    "ceremony {} has accepted participants not in participant list",
                    ceremony_id
                ));
            }
            if state.is_committed && state.has_failed {
                return Err(format!(
                    "ceremony {} cannot be committed and failed",
                    ceremony_id
                ));
            }
            if state.is_superseded && state.is_committed {
                return Err(format!(
                    "ceremony {} cannot be superseded and committed",
                    ceremony_id
                ));
            }
            if state.is_committed && state.accepted_participants.len() < state.threshold_k as usize
            {
                return Err(format!(
                    "ceremony {} committed without reaching threshold",
                    ceremony_id
                ));
            }
        }
        Ok(())
    }
}

/// Internal state of a tracked ceremony.
///
/// This struct holds runtime-specific data for ceremony tracking. For UI display
/// and consistency tracking, convert to `CeremonyStatus` using `to_status()`.
#[derive(Debug, Clone)]
pub struct TrackedCeremony {
    /// Unique ceremony identifier
    pub ceremony_id: CeremonyId,

    /// Ceremony kind
    pub kind: CeremonyKind,

    /// Authority that initiated the ceremony
    pub initiator_id: AuthorityId,

    /// Threshold required for completion (k)
    pub threshold_k: u16,

    /// Total number of participants (n)
    pub total_n: u16,

    /// Participants invited to participate
    pub participants: HashSet<ParticipantIdentity>,

    /// Participants who have accepted
    pub accepted_participants: HashSet<ParticipantIdentity>,

    /// New epoch for the key rotation
    pub new_epoch: u64,

    /// Device being enrolled (DeviceEnrollment ceremonies only).
    pub enrollment_device_id: Option<DeviceId>,

    /// Nickname suggestion for the enrolling device (DeviceEnrollment ceremonies only).
    ///
    /// Stored here to be embedded in `DeviceLeafMetadata` when enrollment completes.
    pub enrollment_nickname_suggestion: Option<String>,

    /// When the ceremony was initiated
    pub started_at: Instant,

    /// Whether the ceremony has failed
    pub has_failed: bool,

    /// Whether the ceremony has been committed (key rotation activated)
    pub is_committed: bool,

    /// Whether the ceremony has been superseded by another ceremony
    pub is_superseded: bool,

    /// ID of the ceremony that supersedes this one (if superseded)
    pub superseded_by: Option<CeremonyId>,

    /// IDs of ceremonies that this ceremony supersedes
    pub supersedes: Vec<CeremonyId>,

    /// Agreement mode (A1/A2/A3) for the ceremony lifecycle
    pub agreement_mode: AgreementMode,

    /// Optional error message if failed
    pub error_message: Option<String>,

    /// Timeout duration (30 seconds default)
    pub timeout: Duration,

    /// Prestate hash at ceremony initiation (for supersession detection)
    pub prestate_hash: Option<Hash32>,

    /// Timestamp when committed (if committed)
    pub committed_at: Option<PhysicalTime>,

    /// Consensus ID when committed (if committed)
    pub committed_consensus_id: Option<ConsensusId>,
}

impl TrackedCeremony {
    /// Convert to `CeremonyStatus` for UI display and consistency tracking.
    pub fn to_status(&self) -> CeremonyStatus {
        // Helper to create a zero physical time (for cases where we don't have the actual time)
        let zero_time = PhysicalTime {
            ts_ms: 0,
            uncertainty: None,
        };
        let zero_consensus_id = ConsensusId::new([0; 32]);

        // Convert internal state to StatusCeremonyState enum
        let state = if self.is_committed {
            StatusCeremonyState::Committed {
                consensus_id: self.committed_consensus_id.unwrap_or(zero_consensus_id),
                committed_at: self.committed_at.clone().unwrap_or(zero_time.clone()),
            }
        } else if self.is_superseded {
            StatusCeremonyState::Superseded {
                by: self
                    .superseded_by
                    .clone()
                    .unwrap_or_else(|| CeremonyId::new("unknown")),
                reason: StatusSupersessionReason::NewerRequest,
            }
        } else if self.has_failed {
            StatusCeremonyState::Aborted {
                reason: self
                    .error_message
                    .clone()
                    .unwrap_or_else(|| "Unknown error".to_string()),
                aborted_at: zero_time.clone(),
            }
        } else if self.accepted_participants.len() >= self.threshold_k as usize {
            StatusCeremonyState::Committing
        } else if !self.accepted_participants.is_empty() {
            StatusCeremonyState::PendingEpoch {
                pending_epoch: aura_core::types::Epoch::new(self.new_epoch),
                required_responses: self.threshold_k,
                received_responses: self.accepted_participants.len() as u16,
            }
        } else {
            StatusCeremonyState::Preparing
        };

        // Convert accepted participants to responses (guardians only for status)
        // Devices and group members are tracked internally but don't map cleanly to AuthorityId
        let responses: Vec<ParticipantResponse> = self
            .accepted_participants
            .iter()
            .filter_map(|p| match p {
                ParticipantIdentity::Guardian(auth_id) => Some(ParticipantResponse {
                    participant: *auth_id,
                    response: CeremonyResponse::Accept,
                    responded_at: zero_time.clone(), // We don't track individual response times
                }),
                ParticipantIdentity::Device(_device_id) => {
                    // Skip devices for status - tracked internally
                    None
                }
                ParticipantIdentity::GroupMember { .. } => {
                    // Skip group members for status - tracked internally
                    None
                }
            })
            .collect();

        // Get committed agreement if applicable
        let committed_agreement = if self.is_committed {
            Some(aura_core::domain::Agreement::Finalized {
                consensus_id: self.committed_consensus_id.unwrap_or(zero_consensus_id),
            })
        } else {
            None
        };

        CeremonyStatus {
            ceremony_id: self.ceremony_id.clone(),
            state,
            responses,
            prestate_hash: self.prestate_hash.unwrap_or(Hash32([0; 32])),
            committed_agreement,
        }
    }
}

impl CeremonyTracker {
    fn initial_mode_for_kind(kind: CeremonyKind) -> AgreementMode {
        match kind {
            CeremonyKind::GuardianRotation => {
                policy_for(CeremonyFlow::GuardianSetupRotation).initial_mode()
            }
            CeremonyKind::DeviceRotation => {
                policy_for(CeremonyFlow::DeviceMfaRotation).initial_mode()
            }
            CeremonyKind::DeviceEnrollment => {
                policy_for(CeremonyFlow::DeviceEnrollment).initial_mode()
            }
            CeremonyKind::DeviceRemoval => policy_for(CeremonyFlow::DeviceRemoval).initial_mode(),
            CeremonyKind::Recovery => policy_for(CeremonyFlow::RecoveryExecution).initial_mode(),
            CeremonyKind::Invitation => policy_for(CeremonyFlow::Invitation).initial_mode(),
        }
    }
    /// Create a new ceremony tracker
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(CeremonyTrackerState::default())),
        }
    }

    /// Register a new ceremony
    ///
    /// # Arguments
    /// * `ceremony_id` - Unique ceremony identifier
    /// * `threshold_k` - Minimum signers required
    /// * `total_n` - Total number of participants
    /// * `participants` - Participants invited
    /// * `new_epoch` - Epoch for the new keys
    #[allow(clippy::too_many_arguments)] // Ceremony registration requires all these distinct parameters
    pub async fn register(
        &self,
        ceremony_id: CeremonyId,
        kind: CeremonyKind,
        initiator_id: AuthorityId,
        threshold_k: u16,
        total_n: u16,
        participants: Vec<ParticipantIdentity>,
        new_epoch: u64,
        enrollment_device_id: Option<DeviceId>,
        enrollment_nickname_suggestion: Option<String>,
    ) -> Result<(), IntentError> {
        self.register_with_prestate(
            ceremony_id,
            kind,
            initiator_id,
            threshold_k,
            total_n,
            participants,
            new_epoch,
            enrollment_device_id,
            enrollment_nickname_suggestion,
            None, // No prestate hash for backward compatibility
        )
        .await
    }

    /// Register a new ceremony with prestate hash for supersession tracking
    #[allow(clippy::too_many_arguments)]
    pub async fn register_with_prestate(
        &self,
        ceremony_id: CeremonyId,
        kind: CeremonyKind,
        initiator_id: AuthorityId,
        threshold_k: u16,
        total_n: u16,
        participants: Vec<ParticipantIdentity>,
        new_epoch: u64,
        enrollment_device_id: Option<DeviceId>,
        enrollment_nickname_suggestion: Option<String>,
        prestate_hash: Option<Hash32>,
    ) -> Result<(), IntentError> {
        let participants_set: HashSet<_> = participants.into_iter().collect();
        if participants_set.len() != total_n as usize {
            return Err(IntentError::validation_failed(format!(
                "Ceremony {} participant count {} does not match total_n {}",
                ceremony_id,
                participants_set.len(),
                total_n
            )));
        }

        let state = TrackedCeremony {
            ceremony_id: ceremony_id.clone(),
            kind,
            initiator_id,
            threshold_k,
            total_n,
            participants: participants_set,
            accepted_participants: HashSet::new(),
            new_epoch,
            enrollment_device_id,
            enrollment_nickname_suggestion,
            started_at: Instant::now(),
            has_failed: false,
            is_committed: false,
            is_superseded: false,
            superseded_by: None,
            supersedes: Vec::new(),
            agreement_mode: Self::initial_mode_for_kind(kind),
            error_message: None,
            timeout: Duration::from_secs(30),
            prestate_hash,
            committed_at: None,
            committed_consensus_id: None,
        };

        let result = with_state_mut_validated(
            &self.state,
            |tracker| {
                if tracker.ceremonies.contains_key(&ceremony_id) {
                    return Err(IntentError::validation_failed(format!(
                        "Ceremony {} already registered",
                        ceremony_id
                    )));
                }
                tracker.ceremonies.insert(ceremony_id.clone(), state);
                Ok(())
            },
            |tracker| tracker.validate(),
        )
        .await;

        if result.is_ok() {
            tracing::info!(
                ceremony_id = %ceremony_id,
                threshold_k,
                total_n,
                "Ceremony registered"
            );
        }

        result
    }

    /// Get ceremony state
    ///
    /// # Arguments
    /// * `ceremony_id` - The ceremony identifier
    ///
    /// # Returns
    /// The current ceremony state
    pub async fn get(&self, ceremony_id: &CeremonyId) -> Result<TrackedCeremony, IntentError> {
        let state = self.state.read().await;

        state.ceremonies.get(ceremony_id).cloned().ok_or_else(|| {
            IntentError::validation_failed(format!("Ceremony {} not found", ceremony_id))
        })
    }

    /// Get ceremony status for UI display
    ///
    /// # Arguments
    /// * `ceremony_id` - The ceremony identifier
    ///
    /// # Returns
    /// The ceremony status for UI display
    pub async fn get_status(
        &self,
        ceremony_id: &CeremonyId,
    ) -> Result<CeremonyStatus, IntentError> {
        self.get(ceremony_id).await.map(|c| c.to_status())
    }

    /// Mark a guardian as having accepted the invitation
    ///
    /// # Arguments
    /// * `ceremony_id` - The ceremony identifier
    /// * `guardian_id` - The guardian who accepted
    ///
    /// # Returns
    /// True if threshold is now reached
    pub async fn mark_accepted(
        &self,
        ceremony_id: &CeremonyId,
        participant: ParticipantIdentity,
    ) -> Result<bool, IntentError> {
        with_state_mut_validated(
            &self.state,
            |tracker| {
                let state = tracker.ceremonies.get_mut(ceremony_id).ok_or_else(|| {
                    IntentError::validation_failed(format!("Ceremony {} not found", ceremony_id))
                })?;

                // Check if participant is part of this ceremony
                if !state.participants.contains(&participant) {
                    return Err(IntentError::validation_failed(format!(
                        "Participant {:?} not part of ceremony {}",
                        participant, ceremony_id
                    )));
                }

                // Check if already accepted
                if state.accepted_participants.contains(&participant) {
                    tracing::debug!(
                        ceremony_id = %ceremony_id,
                        "Participant already accepted (idempotent)"
                    );
                    return Ok(state.accepted_participants.len() >= state.threshold_k as usize);
                }

                // Add to accepted list
                state.accepted_participants.insert(participant.clone());

                let threshold_reached =
                    state.accepted_participants.len() >= state.threshold_k as usize;
                if threshold_reached {
                    state.agreement_mode = AgreementMode::CoordinatorSoftSafe;
                }

                tracing::info!(
                    ceremony_id = %ceremony_id,
                    accepted = state.accepted_participants.len(),
                    threshold = state.threshold_k,
                    threshold_reached,
                    "Participant accepted ceremony"
                );

                Ok(threshold_reached)
            },
            |tracker| tracker.validate(),
        )
        .await
    }

    /// Mark a ceremony as committed (key rotation activated), with optional metadata.
    pub async fn mark_committed_with_metadata(
        &self,
        ceremony_id: &CeremonyId,
        committed_at: Option<PhysicalTime>,
        consensus_id: Option<ConsensusId>,
    ) -> Result<(), IntentError> {
        with_state_mut_validated(
            &self.state,
            |tracker| {
                let state = tracker.ceremonies.get_mut(ceremony_id).ok_or_else(|| {
                    IntentError::validation_failed(format!("Ceremony {} not found", ceremony_id))
                })?;

                if state.is_committed {
                    if let Some(committed_at) = committed_at {
                        state.committed_at = Some(committed_at);
                    }
                    if let Some(consensus_id) = consensus_id {
                        state.committed_consensus_id = Some(consensus_id);
                    }
                    return Ok(());
                }

                state.is_committed = true;
                state.agreement_mode = AgreementMode::ConsensusFinalized;
                if let Some(committed_at) = committed_at {
                    state.committed_at = Some(committed_at);
                }
                if let Some(consensus_id) = consensus_id {
                    state.committed_consensus_id = Some(consensus_id);
                }

                tracing::info!(
                    ceremony_id = %ceremony_id,
                    accepted = state.accepted_participants.len(),
                    threshold = state.threshold_k,
                    "Ceremony committed"
                );

                Ok(())
            },
            |tracker| tracker.validate(),
        )
        .await
    }

    /// Mark a ceremony as committed (key rotation activated).
    ///
    /// This is only called after threshold is reached and `commit_key_rotation` succeeds.
    pub async fn mark_committed(&self, ceremony_id: &CeremonyId) -> Result<(), IntentError> {
        with_state_mut_validated(
            &self.state,
            |tracker| {
                let state = tracker.ceremonies.get_mut(ceremony_id).ok_or_else(|| {
                    IntentError::validation_failed(format!("Ceremony {} not found", ceremony_id))
                })?;

                if state.is_committed {
                    return Ok(());
                }

                state.is_committed = true;
                state.agreement_mode = AgreementMode::ConsensusFinalized;

                tracing::info!(
                    ceremony_id = %ceremony_id,
                    accepted = state.accepted_participants.len(),
                    threshold = state.threshold_k,
                    "Ceremony committed"
                );

                Ok(())
            },
            |tracker| tracker.validate(),
        )
        .await
    }

    /// Check if ceremony is complete (committed)
    ///
    /// # Arguments
    /// * `ceremony_id` - The ceremony identifier
    ///
    /// # Returns
    /// True if threshold is reached
    pub async fn is_complete(&self, ceremony_id: &CeremonyId) -> Result<bool, IntentError> {
        let state = self.get(ceremony_id).await?;
        Ok(state.is_committed)
    }

    /// Check if ceremony has timed out
    ///
    /// # Arguments
    /// * `ceremony_id` - The ceremony identifier
    ///
    /// # Returns
    /// True if ceremony has exceeded its timeout
    pub async fn is_timed_out(&self, ceremony_id: &CeremonyId) -> Result<bool, IntentError> {
        let state = self.get(ceremony_id).await?;
        Ok(state.started_at.elapsed() > state.timeout)
    }

    /// Mark ceremony as failed
    ///
    /// # Arguments
    /// * `ceremony_id` - The ceremony identifier
    /// * `error_message` - Optional error description
    pub async fn mark_failed(
        &self,
        ceremony_id: &CeremonyId,
        error_message: Option<String>,
    ) -> Result<(), IntentError> {
        with_state_mut_validated(
            &self.state,
            |tracker| {
                let state = tracker.ceremonies.get_mut(ceremony_id).ok_or_else(|| {
                    IntentError::validation_failed(format!("Ceremony {} not found", ceremony_id))
                })?;

                state.has_failed = true;
                state.error_message = error_message.clone();

                tracing::warn!(
                    ceremony_id = %ceremony_id,
                    error = ?error_message,
                    "Ceremony marked as failed"
                );

                Ok(())
            },
            |tracker| tracker.validate(),
        )
        .await
    }

    /// Remove ceremony from tracker (cleanup after completion/failure)
    ///
    /// # Arguments
    /// * `ceremony_id` - The ceremony identifier
    pub async fn remove(&self, ceremony_id: &CeremonyId) -> Result<(), IntentError> {
        with_state_mut_validated(
            &self.state,
            |tracker| {
                tracker.ceremonies.remove(ceremony_id).ok_or_else(|| {
                    IntentError::validation_failed(format!("Ceremony {} not found", ceremony_id))
                })?;

                tracing::debug!(ceremony_id = %ceremony_id, "Ceremony removed from tracker");

                Ok(())
            },
            |tracker| tracker.validate(),
        )
        .await
    }

    /// Get list of all active ceremonies
    ///
    /// # Returns
    /// Vector of (ceremony_id, state) tuples
    pub async fn list_active(&self) -> Vec<(CeremonyId, TrackedCeremony)> {
        let state = self.state.read().await;
        state
            .ceremonies
            .iter()
            .map(|(id, ceremony)| (id.clone(), ceremony.clone()))
            .collect()
    }

    /// Cleanup timed out ceremonies
    ///
    /// Should be called periodically to remove stale ceremonies
    ///
    /// # Returns
    /// Number of ceremonies cleaned up
    pub async fn cleanup_timed_out(&self) -> usize {
        with_state_mut_validated(
            &self.state,
            |tracker| {
                let mut removed = Vec::new();

                for (id, state) in tracker.ceremonies.iter() {
                    if state.started_at.elapsed() > state.timeout && !state.has_failed {
                        removed.push(id.clone());
                    }
                }

                for id in &removed {
                    if let Some(state) = tracker.ceremonies.get_mut(id) {
                        state.has_failed = true;
                        state.error_message = Some("Ceremony timed out".to_string());
                    }
                    tracing::warn!(
                        ceremony_id = %id,
                        "Ceremony timed out and marked as failed"
                    );
                }

                removed.len()
            },
            |tracker| tracker.validate(),
        )
        .await
    }

    // =========================================================================
    // SUPERSESSION METHODS
    // =========================================================================

    /// Supersede an existing ceremony with a new one.
    ///
    /// The old ceremony is marked as superseded and should stop processing.
    /// Supersession facts should be emitted after calling this method.
    ///
    /// # Arguments
    /// * `old_ceremony_id` - The ceremony being superseded
    /// * `new_ceremony_id` - The ceremony that supersedes it
    /// * `reason` - Why the supersession occurred
    /// * `timestamp_ms` - When the supersession was recorded
    pub async fn supersede(
        &self,
        old_ceremony_id: &CeremonyId,
        new_ceremony_id: &CeremonyId,
        reason: SupersessionReason,
        timestamp_ms: u64,
    ) -> Result<SupersessionRecord, IntentError> {
        // Create record outside the lock scope
        let old_ceremony_hash = Hash32::from_bytes(old_ceremony_id.as_str().as_bytes());
        let new_ceremony_hash = Hash32::from_bytes(new_ceremony_id.as_str().as_bytes());
        let record = SupersessionRecord::new(
            old_ceremony_hash,
            new_ceremony_hash,
            reason.clone(),
            timestamp_ms,
        );

        with_state_mut_validated(
            &self.state,
            |tracker| {
                // Verify old ceremony exists
                let old_state = tracker.ceremonies.get_mut(old_ceremony_id).ok_or_else(|| {
                    IntentError::validation_failed(format!(
                        "Ceremony {} not found",
                        old_ceremony_id
                    ))
                })?;

                // Check if already in terminal state
                if old_state.is_committed {
                    return Err(IntentError::validation_failed(format!(
                        "Cannot supersede committed ceremony {}",
                        old_ceremony_id
                    )));
                }

                if old_state.is_superseded {
                    // Already superseded - idempotent
                    tracing::debug!(
                        old_ceremony = %old_ceremony_id,
                        new_ceremony = %new_ceremony_id,
                        "Ceremony already superseded (idempotent)"
                    );
                    return Ok(record.clone());
                }

                // Mark old ceremony as superseded
                old_state.is_superseded = true;
                old_state.superseded_by = Some(new_ceremony_id.clone());
                old_state.has_failed = true;
                old_state.error_message = Some(format!("Superseded: {}", reason.description()));

                // Update new ceremony if it exists (may be registered separately)
                if let Some(new_state) = tracker.ceremonies.get_mut(new_ceremony_id) {
                    new_state.supersedes.push(old_ceremony_id.clone());
                }

                // Record for audit trail
                tracker.supersession_records.push(record.clone());

                tracing::info!(
                    old_ceremony = %old_ceremony_id,
                    new_ceremony = %new_ceremony_id,
                    reason = %reason.code(),
                    "Ceremony superseded"
                );

                Ok(record.clone())
            },
            |tracker| tracker.validate(),
        )
        .await
    }

    /// Check for ceremonies that would be superseded by a new ceremony.
    ///
    /// Returns active ceremonies of the same kind that could be superseded
    /// based on prestate staleness or same-initiator detection.
    ///
    /// # Arguments
    /// * `kind` - The kind of ceremony being initiated
    /// * `prestate_hash` - Current prestate hash (if available)
    ///
    /// # Returns
    /// Vector of ceremony IDs that are candidates for supersession
    pub async fn check_supersession_candidates(
        &self,
        kind: CeremonyKind,
        prestate_hash: Option<&Hash32>,
    ) -> Vec<CeremonyId> {
        let state = self.state.read().await;

        state
            .ceremonies
            .iter()
            .filter(|(_, ceremony)| {
                // Must be same kind
                if ceremony.kind != kind {
                    return false;
                }

                // Skip terminal ceremonies
                if ceremony.is_committed || ceremony.is_superseded {
                    return false;
                }

                // Check for prestate staleness if we have both hashes
                if let (Some(new_hash), Some(old_hash)) = (prestate_hash, &ceremony.prestate_hash) {
                    if new_hash != old_hash {
                        return true; // Prestate changed, candidate for supersession
                    }
                }

                // Active ceremony of same kind is always a candidate
                true
            })
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Get supersession chain for audit trail.
    ///
    /// Returns all supersession records involving the given ceremony
    /// (either as superseded or superseding).
    ///
    /// # Arguments
    /// * `ceremony_id` - The ceremony to get supersession history for
    pub async fn get_supersession_chain(
        &self,
        ceremony_id: &CeremonyId,
    ) -> Vec<SupersessionRecord> {
        let ceremony_hash = Hash32::from_bytes(ceremony_id.as_str().as_bytes());
        let state = self.state.read().await;

        state
            .supersession_records
            .iter()
            .filter(|record| {
                record.superseded_id == ceremony_hash || record.superseding_id == ceremony_hash
            })
            .cloned()
            .collect()
    }

    /// Check if a ceremony has been superseded.
    pub async fn is_superseded(&self, ceremony_id: &CeremonyId) -> Result<bool, IntentError> {
        let state = self.get(ceremony_id).await?;
        Ok(state.is_superseded)
    }

    /// Get all supersession records (for debugging/auditing).
    pub async fn all_supersession_records(&self) -> Vec<SupersessionRecord> {
        let state = self.state.read().await;
        state.supersession_records.clone()
    }
}

impl Default for CeremonyTracker {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// RuntimeService Implementation
// =============================================================================

use super::traits::{RuntimeService, ServiceError, ServiceHealth};
use super::RuntimeTaskRegistry;
use async_trait::async_trait;

#[async_trait]
impl RuntimeService for CeremonyTracker {
    fn name(&self) -> &'static str {
        "ceremony_tracker"
    }

    async fn start(&self, _tasks: Arc<RuntimeTaskRegistry>) -> Result<(), ServiceError> {
        // CeremonyTracker is in-memory and always ready
        Ok(())
    }

    async fn stop(&self) -> Result<(), ServiceError> {
        // Clean up any tracked ceremonies
        self.cleanup_timed_out().await;
        Ok(())
    }

    fn health(&self) -> ServiceHealth {
        ServiceHealth::Healthy
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::identifiers::{AuthorityId, CeremonyId};
    use aura_core::DeviceId;

    fn test_ceremony_id(label: &str) -> CeremonyId {
        CeremonyId::new(label)
    }

    #[tokio::test]
    async fn test_ceremony_registration() {
        let tracker = CeremonyTracker::new();

        let ceremony_id = test_ceremony_id("ceremony-1");
        let a = AuthorityId::new_from_entropy([1u8; 32]);
        let b = AuthorityId::new_from_entropy([2u8; 32]);
        let c = AuthorityId::new_from_entropy([3u8; 32]);

        tracker
            .register(
                ceremony_id.clone(),
                CeremonyKind::GuardianRotation,
                a,
                2,
                3,
                vec![
                    ParticipantIdentity::guardian(a),
                    ParticipantIdentity::guardian(b),
                    ParticipantIdentity::guardian(c),
                ],
                100,
                None,
                None,
            )
            .await
            .unwrap();

        let state = tracker.get(&ceremony_id).await.unwrap();
        assert_eq!(state.threshold_k, 2);
        assert_eq!(state.total_n, 3);
        assert_eq!(state.participants.len(), 3);
        assert_eq!(state.accepted_participants.len(), 0);
    }

    #[tokio::test]
    async fn test_guardian_acceptance() {
        let tracker = CeremonyTracker::new();

        let ceremony_id = test_ceremony_id("ceremony-1");
        let a = AuthorityId::new_from_entropy([1u8; 32]);
        let b = AuthorityId::new_from_entropy([2u8; 32]);
        let c = AuthorityId::new_from_entropy([3u8; 32]);

        tracker
            .register(
                ceremony_id.clone(),
                CeremonyKind::GuardianRotation,
                a,
                2,
                3,
                vec![
                    ParticipantIdentity::guardian(a),
                    ParticipantIdentity::guardian(b),
                    ParticipantIdentity::guardian(c),
                ],
                100,
                None,
                None,
            )
            .await
            .unwrap();

        // First acceptance
        let threshold_reached = tracker
            .mark_accepted(&ceremony_id, ParticipantIdentity::guardian(a))
            .await
            .unwrap();
        assert!(!threshold_reached);

        // Second acceptance - threshold reached
        let threshold_reached = tracker
            .mark_accepted(&ceremony_id, ParticipantIdentity::guardian(b))
            .await
            .unwrap();
        assert!(threshold_reached);

        let state = tracker.get(&ceremony_id).await.unwrap();
        assert_eq!(state.accepted_participants.len(), 2);
        assert!(state
            .accepted_participants
            .contains(&ParticipantIdentity::guardian(a)));
        assert!(state
            .accepted_participants
            .contains(&ParticipantIdentity::guardian(b)));
    }

    #[tokio::test]
    async fn test_ceremony_completion() {
        let tracker = CeremonyTracker::new();

        let ceremony_id = test_ceremony_id("ceremony-1");
        let a = AuthorityId::new_from_entropy([1u8; 32]);
        let b = AuthorityId::new_from_entropy([2u8; 32]);
        let c = AuthorityId::new_from_entropy([3u8; 32]);

        tracker
            .register(
                ceremony_id.clone(),
                CeremonyKind::GuardianRotation,
                a,
                2,
                3,
                vec![
                    ParticipantIdentity::guardian(a),
                    ParticipantIdentity::guardian(b),
                    ParticipantIdentity::guardian(c),
                ],
                100,
                None,
                None,
            )
            .await
            .unwrap();

        assert!(!tracker.is_complete(&ceremony_id).await.unwrap());

        tracker
            .mark_accepted(&ceremony_id, ParticipantIdentity::guardian(a))
            .await
            .unwrap();
        assert!(!tracker.is_complete(&ceremony_id).await.unwrap());

        let threshold_reached = tracker
            .mark_accepted(&ceremony_id, ParticipantIdentity::guardian(b))
            .await
            .unwrap();
        assert!(threshold_reached);

        // Completion is only true once the key rotation is committed.
        assert!(!tracker.is_complete(&ceremony_id).await.unwrap());
        tracker.mark_committed(&ceremony_id).await.unwrap();
        assert!(tracker.is_complete(&ceremony_id).await.unwrap());
    }

    #[tokio::test]
    async fn test_agreement_mode_transitions() {
        let tracker = CeremonyTracker::new();

        let ceremony_id = test_ceremony_id("ceremony-1");
        let a = AuthorityId::new_from_entropy([1u8; 32]);
        let b = AuthorityId::new_from_entropy([2u8; 32]);

        tracker
            .register(
                ceremony_id.clone(),
                CeremonyKind::GuardianRotation,
                a,
                2,
                2,
                vec![
                    ParticipantIdentity::guardian(a),
                    ParticipantIdentity::guardian(b),
                ],
                100,
                None,
                None,
            )
            .await
            .unwrap();

        let state = tracker.get(&ceremony_id).await.unwrap();
        assert_eq!(state.agreement_mode, AgreementMode::CoordinatorSoftSafe);

        tracker
            .mark_accepted(&ceremony_id, ParticipantIdentity::guardian(a))
            .await
            .unwrap();
        tracker
            .mark_accepted(&ceremony_id, ParticipantIdentity::guardian(b))
            .await
            .unwrap();

        let state = tracker.get(&ceremony_id).await.unwrap();
        assert_eq!(state.agreement_mode, AgreementMode::CoordinatorSoftSafe);

        tracker.mark_committed(&ceremony_id).await.unwrap();
        let state = tracker.get(&ceremony_id).await.unwrap();
        assert_eq!(state.agreement_mode, AgreementMode::ConsensusFinalized);
    }

    #[tokio::test]
    async fn test_idempotent_acceptance() {
        let tracker = CeremonyTracker::new();

        let ceremony_id = test_ceremony_id("ceremony-1");
        let a = AuthorityId::new_from_entropy([1u8; 32]);
        let b = AuthorityId::new_from_entropy([2u8; 32]);
        let c = AuthorityId::new_from_entropy([3u8; 32]);

        tracker
            .register(
                ceremony_id.clone(),
                CeremonyKind::GuardianRotation,
                a,
                2,
                3,
                vec![
                    ParticipantIdentity::guardian(a),
                    ParticipantIdentity::guardian(b),
                    ParticipantIdentity::guardian(c),
                ],
                100,
                None,
                None,
            )
            .await
            .unwrap();

        // Accept twice
        tracker
            .mark_accepted(&ceremony_id, ParticipantIdentity::guardian(a))
            .await
            .unwrap();
        tracker
            .mark_accepted(&ceremony_id, ParticipantIdentity::guardian(a))
            .await
            .unwrap();

        let state = tracker.get(&ceremony_id).await.unwrap();
        assert_eq!(state.accepted_participants.len(), 1);
    }

    #[tokio::test]
    async fn test_ceremony_failure() {
        let tracker = CeremonyTracker::new();

        let ceremony_id = test_ceremony_id("ceremony-1");
        let a = AuthorityId::new_from_entropy([1u8; 32]);
        let b = AuthorityId::new_from_entropy([2u8; 32]);
        let c = AuthorityId::new_from_entropy([3u8; 32]);

        tracker
            .register(
                ceremony_id.clone(),
                CeremonyKind::GuardianRotation,
                a,
                2,
                3,
                vec![
                    ParticipantIdentity::guardian(a),
                    ParticipantIdentity::guardian(b),
                    ParticipantIdentity::guardian(c),
                ],
                100,
                None,
                None,
            )
            .await
            .unwrap();

        tracker
            .mark_failed(&ceremony_id, Some("Test failure".to_string()))
            .await
            .unwrap();

        let state = tracker.get(&ceremony_id).await.unwrap();
        assert!(state.has_failed);
        assert_eq!(state.error_message, Some("Test failure".to_string()));
    }

    #[tokio::test]
    async fn test_device_enrollment_ceremony_acceptance() {
        let tracker = CeremonyTracker::new();
        let device = DeviceId::new_from_entropy([9u8; 32]);
        let initiator = AuthorityId::new_from_entropy([1u8; 32]);
        let ceremony_id = test_ceremony_id("ceremony-device-1");

        tracker
            .register(
                ceremony_id.clone(),
                CeremonyKind::DeviceEnrollment,
                initiator,
                1,
                1,
                vec![ParticipantIdentity::device(device)],
                42,
                Some(device),
                Some("My Test Device".to_string()),
            )
            .await
            .unwrap();

        let state = tracker.get(&ceremony_id).await.unwrap();
        assert_eq!(state.kind, CeremonyKind::DeviceEnrollment);
        assert_eq!(state.threshold_k, 1);
        assert_eq!(state.total_n, 1);

        let threshold_reached = tracker
            .mark_accepted(&ceremony_id, ParticipantIdentity::device(device))
            .await
            .unwrap();
        assert!(threshold_reached);

        tracker.mark_committed(&ceremony_id).await.unwrap();
        assert!(tracker.is_complete(&ceremony_id).await.unwrap());
    }

    #[tokio::test]
    async fn test_device_rotation_agreement_mode_transitions() {
        let tracker = CeremonyTracker::new();
        let device_a = DeviceId::new_from_entropy([10u8; 32]);
        let device_b = DeviceId::new_from_entropy([11u8; 32]);
        let initiator = AuthorityId::new_from_entropy([2u8; 32]);
        let ceremony_id = test_ceremony_id("ceremony-rotate-1");

        tracker
            .register(
                ceremony_id.clone(),
                CeremonyKind::DeviceRotation,
                initiator,
                2,
                2,
                vec![
                    ParticipantIdentity::device(device_a),
                    ParticipantIdentity::device(device_b),
                ],
                77,
                None,
                None,
            )
            .await
            .unwrap();

        let state = tracker.get(&ceremony_id).await.unwrap();
        assert_eq!(state.agreement_mode, AgreementMode::CoordinatorSoftSafe);

        tracker
            .mark_accepted(&ceremony_id, ParticipantIdentity::device(device_a))
            .await
            .unwrap();
        tracker
            .mark_accepted(&ceremony_id, ParticipantIdentity::device(device_b))
            .await
            .unwrap();

        tracker.mark_committed(&ceremony_id).await.unwrap();
        let state = tracker.get(&ceremony_id).await.unwrap();
        assert_eq!(state.agreement_mode, AgreementMode::ConsensusFinalized);
    }

    #[tokio::test]
    async fn test_device_removal_agreement_mode_transitions() {
        let tracker = CeremonyTracker::new();
        let device_a = DeviceId::new_from_entropy([12u8; 32]);
        let device_b = DeviceId::new_from_entropy([13u8; 32]);
        let initiator = AuthorityId::new_from_entropy([3u8; 32]);
        let ceremony_id = test_ceremony_id("ceremony-remove-1");

        tracker
            .register(
                ceremony_id.clone(),
                CeremonyKind::DeviceRemoval,
                initiator,
                2,
                2,
                vec![
                    ParticipantIdentity::device(device_a),
                    ParticipantIdentity::device(device_b),
                ],
                88,
                Some(device_b),
                None,
            )
            .await
            .unwrap();

        let state = tracker.get(&ceremony_id).await.unwrap();
        assert_eq!(state.agreement_mode, AgreementMode::CoordinatorSoftSafe);

        tracker
            .mark_accepted(&ceremony_id, ParticipantIdentity::device(device_a))
            .await
            .unwrap();
        tracker
            .mark_accepted(&ceremony_id, ParticipantIdentity::device(device_b))
            .await
            .unwrap();

        tracker.mark_committed(&ceremony_id).await.unwrap();
        let state = tracker.get(&ceremony_id).await.unwrap();
        assert_eq!(state.agreement_mode, AgreementMode::ConsensusFinalized);
    }

    // =========================================================================
    // PROPERTY TESTS
    // =========================================================================

    use proptest::prelude::*;

    /// Strategy to generate a valid TrackedCeremony
    #[allow(dead_code)] // Reserved for future proptest expansion
    fn tracked_ceremony_strategy() -> impl Strategy<Value = TrackedCeremony> {
        (
            2usize..=8, // num_participants
            1u16..=8,   // threshold (will be clamped)
        )
            .prop_flat_map(|(num_participants, threshold)| {
                let threshold = threshold.min(num_participants as u16);
                let participants: Vec<ParticipantIdentity> = (0..num_participants)
                    .map(|i| {
                        ParticipantIdentity::guardian(AuthorityId::new_from_entropy([i as u8; 32]))
                    })
                    .collect();

                // Generate a subset of participants to be accepted
                let num_accepted = 0..=num_participants;

                (Just(participants), Just(threshold), num_accepted)
            })
            .prop_map(|(participants, threshold, num_accepted)| {
                let accepted: HashSet<_> =
                    participants.iter().take(num_accepted).cloned().collect();
                let participants_set: HashSet<_> = participants.into_iter().collect();

                TrackedCeremony {
                    ceremony_id: CeremonyId::new("proptest"),
                    kind: CeremonyKind::GuardianRotation,
                    initiator_id: AuthorityId::new_from_entropy([0u8; 32]),
                    threshold_k: threshold,
                    total_n: participants_set.len() as u16,
                    participants: participants_set,
                    accepted_participants: accepted,
                    new_epoch: 100,
                    enrollment_device_id: None,
                    enrollment_nickname_suggestion: None,
                    started_at: Instant::now(),
                    has_failed: false,
                    is_committed: false,
                    is_superseded: false,
                    superseded_by: None,
                    supersedes: Vec::new(),
                    agreement_mode: AgreementMode::CoordinatorSoftSafe,
                    error_message: None,
                    timeout: Duration::from_secs(30),
                    prestate_hash: None,
                    committed_at: None,
                    committed_consensus_id: None,
                }
            })
    }

    proptest! {
        /// Property: Participants list has no duplicates.
        /// This invariant is enforced by the validate() function.
        #[test]
        fn prop_no_duplicate_participants(
            num_participants in 2usize..=8,
        ) {
            let participants: Vec<ParticipantIdentity> = (0..num_participants)
                .map(|i| ParticipantIdentity::guardian(AuthorityId::new_from_entropy([i as u8; 32])))
                .collect();
            let participants_set: HashSet<_> = participants.iter().cloned().collect();

            let state = CeremonyTrackerState {
                ceremonies: {
                    let mut map = HashMap::new();
                    map.insert(test_ceremony_id("test"), TrackedCeremony {
                        ceremony_id: test_ceremony_id("test"),
                        kind: CeremonyKind::GuardianRotation,
                        initiator_id: AuthorityId::new_from_entropy([0u8; 32]),
                        threshold_k: 1,
                        total_n: participants_set.len() as u16,
                        participants: participants_set,
                        accepted_participants: HashSet::new(),
                        new_epoch: 100,
                        enrollment_device_id: None,
                        enrollment_nickname_suggestion: None,
                        started_at: Instant::now(),
                        has_failed: false,
                        is_committed: false,
                        is_superseded: false,
                        superseded_by: None,
                        supersedes: Vec::new(),
                        agreement_mode: AgreementMode::CoordinatorSoftSafe,
                        error_message: None,
                        timeout: Duration::from_secs(30),
                        prestate_hash: None,
                        committed_at: None,
                        committed_consensus_id: None,
                    });
                    map
                },
                supersession_records: Vec::new(),
            };

            // Unique participants should pass validation
            prop_assert!(state.validate().is_ok());

            // Verify HashSet uniqueness invariant
            let participant_set: HashSet<_> = participants.iter().collect();
            prop_assert_eq!(participant_set.len(), participants.len());
        }

        /// Property: Accepted participants is always a subset of participants.
        #[test]
        fn prop_accepted_subset_of_participants(
            num_participants in 2usize..=8,
            num_accepted in 0usize..=8
        ) {
            let participants: Vec<ParticipantIdentity> = (0..num_participants)
                .map(|i| ParticipantIdentity::guardian(AuthorityId::new_from_entropy([i as u8; 32])))
                .collect();
            let participants_set: HashSet<_> = participants.iter().cloned().collect();

            // Take a valid subset of participants as accepted
            let num_accepted = num_accepted.min(num_participants);
            let accepted: HashSet<_> = participants.iter().take(num_accepted).cloned().collect();

            let state = CeremonyTrackerState {
                ceremonies: {
                    let mut map = HashMap::new();
                    map.insert(test_ceremony_id("test"), TrackedCeremony {
                        ceremony_id: test_ceremony_id("test"),
                        kind: CeremonyKind::GuardianRotation,
                        initiator_id: AuthorityId::new_from_entropy([0u8; 32]),
                        threshold_k: 1,
                        total_n: num_participants as u16,
                        participants: participants_set.clone(),
                        accepted_participants: accepted.clone(),
                        new_epoch: 100,
                        enrollment_device_id: None,
                        enrollment_nickname_suggestion: None,
                        started_at: Instant::now(),
                        has_failed: false,
                        is_committed: false,
                        is_superseded: false,
                        superseded_by: None,
                        supersedes: Vec::new(),
                        agreement_mode: AgreementMode::CoordinatorSoftSafe,
                        error_message: None,
                        timeout: Duration::from_secs(30),
                        prestate_hash: None,
                        committed_at: None,
                        committed_consensus_id: None,
                    });
                    map
                },
                supersession_records: Vec::new(),
            };

            // Valid subset should pass validation
            prop_assert!(state.validate().is_ok());

            // Verify subset relationship
            prop_assert!(accepted.is_subset(&participants_set));
        }

        /// Property: Threshold must be <= total participants.
        #[test]
        fn prop_threshold_within_bounds(
            num_participants in 1usize..=8,
            threshold in 1u16..=8
        ) {
            let participants: Vec<ParticipantIdentity> = (0..num_participants)
                .map(|i| ParticipantIdentity::guardian(AuthorityId::new_from_entropy([i as u8; 32])))
                .collect();
            let participants_set: HashSet<_> = participants.iter().cloned().collect();

            let state = CeremonyTrackerState {
                ceremonies: {
                    let mut map = HashMap::new();
                    map.insert(test_ceremony_id("test"), TrackedCeremony {
                        ceremony_id: test_ceremony_id("test"),
                        kind: CeremonyKind::GuardianRotation,
                        initiator_id: AuthorityId::new_from_entropy([0u8; 32]),
                        threshold_k: threshold,
                        total_n: num_participants as u16,
                        participants: participants_set,
                        accepted_participants: HashSet::new(),
                        new_epoch: 100,
                        enrollment_device_id: None,
                        enrollment_nickname_suggestion: None,
                        started_at: Instant::now(),
                        has_failed: false,
                        is_committed: false,
                        is_superseded: false,
                        superseded_by: None,
                        supersedes: Vec::new(),
                        agreement_mode: AgreementMode::CoordinatorSoftSafe,
                        error_message: None,
                        timeout: Duration::from_secs(30),
                        prestate_hash: None,
                        committed_at: None,
                        committed_consensus_id: None,
                    });
                    map
                },
                supersession_records: Vec::new(),
            };

            let result = state.validate();

            // Should succeed iff threshold <= num_participants
            if threshold as usize <= num_participants {
                prop_assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
            } else {
                prop_assert!(result.is_err(), "Expected Err for threshold {} > total {}", threshold, num_participants);
            }
        }

        /// Property: Committed ceremonies must have threshold met.
        /// is_committed implies accepted_participants.len() >= threshold_k
        #[test]
        fn prop_committed_implies_threshold_met(
            num_participants in 2usize..=8,
            threshold in 1u16..=8,
            num_accepted in 0usize..=8
        ) {
            let threshold = threshold.min(num_participants as u16);
            let num_accepted = num_accepted.min(num_participants);

            let participants: Vec<ParticipantIdentity> = (0..num_participants)
                .map(|i| ParticipantIdentity::guardian(AuthorityId::new_from_entropy([i as u8; 32])))
                .collect();
            let accepted: HashSet<_> = participants.iter().take(num_accepted).cloned().collect();
            let participants_set: HashSet<_> = participants.into_iter().collect();

            let threshold_met = num_accepted >= threshold as usize;

            let state = CeremonyTrackerState {
                ceremonies: {
                    let mut map = HashMap::new();
                    map.insert(test_ceremony_id("test"), TrackedCeremony {
                        ceremony_id: test_ceremony_id("test"),
                        kind: CeremonyKind::GuardianRotation,
                        initiator_id: AuthorityId::new_from_entropy([0u8; 32]),
                        threshold_k: threshold,
                        total_n: num_participants as u16,
                        participants: participants_set,
                        accepted_participants: accepted,
                        new_epoch: 100,
                        enrollment_device_id: None,
                        enrollment_nickname_suggestion: None,
                        started_at: Instant::now(),
                        has_failed: false,
                        is_committed: true, // Mark as committed
                        is_superseded: false,
                        superseded_by: None,
                        supersedes: Vec::new(),
                        agreement_mode: AgreementMode::ConsensusFinalized,
                        error_message: None,
                        timeout: Duration::from_secs(30),
                        prestate_hash: None,
                        committed_at: None,
                        committed_consensus_id: None,
                    });
                    map
                },
                supersession_records: Vec::new(),
            };

            let result = state.validate();

            // Should succeed iff threshold is met
            if threshold_met {
                prop_assert!(result.is_ok(), "Expected Ok when threshold met, got: {:?}", result);
            } else {
                prop_assert!(result.is_err(), "Expected Err for committed ceremony without threshold");
            }
        }

        /// Property: Committed and failed are mutually exclusive.
        #[test]
        fn prop_committed_and_failed_mutually_exclusive(
            is_committed in any::<bool>(),
            has_failed in any::<bool>()
        ) {
            let a = AuthorityId::new_from_entropy([1u8; 32]);
            let b = AuthorityId::new_from_entropy([2u8; 32]);
            let participants = vec![
                ParticipantIdentity::guardian(a),
                ParticipantIdentity::guardian(b),
            ];
            let accepted: HashSet<_> = participants.iter().cloned().collect(); // All accepted for threshold
            let participants_set: HashSet<_> = participants.into_iter().collect();

            let state = CeremonyTrackerState {
                ceremonies: {
                    let mut map = HashMap::new();
                    map.insert(test_ceremony_id("test"), TrackedCeremony {
                        ceremony_id: test_ceremony_id("test"),
                        kind: CeremonyKind::GuardianRotation,
                        initiator_id: AuthorityId::new_from_entropy([0u8; 32]),
                        threshold_k: 2,
                        total_n: 2,
                        participants: participants_set,
                        accepted_participants: accepted,
                        new_epoch: 100,
                        enrollment_device_id: None,
                        enrollment_nickname_suggestion: None,
                        started_at: Instant::now(),
                        has_failed,
                        is_committed,
                        is_superseded: false,
                        superseded_by: None,
                        supersedes: Vec::new(),
                        agreement_mode: if is_committed {
                            AgreementMode::ConsensusFinalized
                        } else {
                            AgreementMode::CoordinatorSoftSafe
                        },
                        error_message: if has_failed { Some("test".to_string()) } else { None },
                        timeout: Duration::from_secs(30),
                        prestate_hash: None,
                        committed_at: None,
                        committed_consensus_id: None,
                    });
                    map
                },
                supersession_records: Vec::new(),
            };

            let result = state.validate();

            // Both true => error
            if is_committed && has_failed {
                prop_assert!(result.is_err());
            }
        }

        /// Property: Superseded and committed are mutually exclusive.
        #[test]
        fn prop_superseded_and_committed_mutually_exclusive(
            is_committed in any::<bool>(),
            is_superseded in any::<bool>()
        ) {
            let a = AuthorityId::new_from_entropy([1u8; 32]);
            let b = AuthorityId::new_from_entropy([2u8; 32]);
            let participants = vec![
                ParticipantIdentity::guardian(a),
                ParticipantIdentity::guardian(b),
            ];
            let accepted: HashSet<_> = participants.iter().cloned().collect(); // All accepted for threshold
            let participants_set: HashSet<_> = participants.into_iter().collect();

            let state = CeremonyTrackerState {
                ceremonies: {
                    let mut map = HashMap::new();
                    map.insert(test_ceremony_id("test"), TrackedCeremony {
                        ceremony_id: test_ceremony_id("test"),
                        kind: CeremonyKind::GuardianRotation,
                        initiator_id: AuthorityId::new_from_entropy([0u8; 32]),
                        threshold_k: 2,
                        total_n: 2,
                        participants: participants_set,
                        accepted_participants: accepted,
                        new_epoch: 100,
                        enrollment_device_id: None,
                        enrollment_nickname_suggestion: None,
                        started_at: Instant::now(),
                        has_failed: is_superseded, // Superseded ceremonies are marked failed
                        is_committed,
                        is_superseded,
                        superseded_by: if is_superseded {
                            Some(test_ceremony_id("other"))
                        } else {
                            None
                        },
                        supersedes: Vec::new(),
                        agreement_mode: if is_committed {
                            AgreementMode::ConsensusFinalized
                        } else {
                            AgreementMode::CoordinatorSoftSafe
                        },
                        error_message: None,
                        timeout: Duration::from_secs(30),
                        prestate_hash: None,
                        committed_at: None,
                        committed_consensus_id: None,
                    });
                    map
                },
                supersession_records: Vec::new(),
            };

            let result = state.validate();

            // Both true => error
            if is_committed && is_superseded {
                prop_assert!(result.is_err());
            }
        }
    }
}
