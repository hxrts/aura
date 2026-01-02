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

use super::state::with_state_mut_validated;
use aura_app::core::IntentError;
use aura_app::runtime_bridge::CeremonyKind;
use aura_core::ceremony::{SupersessionReason, SupersessionRecord};
use aura_core::threshold::{policy_for, AgreementMode, CeremonyFlow, ParticipantIdentity};
use aura_core::DeviceId;
use aura_core::Hash32;
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
    ceremonies: HashMap<String, CeremonyState>,
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
            let participant_set: HashSet<_> = state.participants.iter().collect();
            if participant_set.len() != state.participants.len() {
                return Err(format!(
                    "ceremony {} has duplicate participants",
                    ceremony_id
                ));
            }
            let accepted_set: HashSet<_> = state.accepted_participants.iter().collect();
            if accepted_set.len() != state.accepted_participants.len() {
                return Err(format!(
                    "ceremony {} has duplicate accepted participants",
                    ceremony_id
                ));
            }
            if !accepted_set.is_subset(&participant_set) {
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

/// State of a guardian ceremony
#[derive(Debug, Clone)]
pub struct CeremonyState {
    /// Ceremony kind
    pub kind: CeremonyKind,

    /// Threshold required for completion (k)
    pub threshold_k: u16,

    /// Total number of participants (n)
    pub total_n: u16,

    /// Participants invited to participate
    pub participants: Vec<ParticipantIdentity>,

    /// Participants who have accepted
    pub accepted_participants: Vec<ParticipantIdentity>,

    /// New epoch for the key rotation
    pub new_epoch: u64,

    /// Device being enrolled (DeviceEnrollment ceremonies only).
    pub enrollment_device_id: Option<DeviceId>,

    /// When the ceremony was initiated
    pub started_at: Instant,

    /// Whether the ceremony has failed
    pub has_failed: bool,

    /// Whether the ceremony has been committed (key rotation activated)
    pub is_committed: bool,

    /// Whether the ceremony has been superseded by another ceremony
    pub is_superseded: bool,

    /// ID of the ceremony that supersedes this one (if superseded)
    pub superseded_by: Option<String>,

    /// IDs of ceremonies that this ceremony supersedes
    pub supersedes: Vec<String>,

    /// Agreement mode (A1/A2/A3) for the ceremony lifecycle
    pub agreement_mode: AgreementMode,

    /// Optional error message if failed
    pub error_message: Option<String>,

    /// Timeout duration (30 seconds default)
    pub timeout: Duration,

    /// Prestate hash at ceremony initiation (for supersession detection)
    pub prestate_hash: Option<Hash32>,
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
        ceremony_id: String,
        kind: CeremonyKind,
        threshold_k: u16,
        total_n: u16,
        participants: Vec<ParticipantIdentity>,
        new_epoch: u64,
        enrollment_device_id: Option<DeviceId>,
    ) -> Result<(), IntentError> {
        self.register_with_prestate(
            ceremony_id,
            kind,
            threshold_k,
            total_n,
            participants,
            new_epoch,
            enrollment_device_id,
            None, // No prestate hash for backward compatibility
        )
        .await
    }

    /// Register a new ceremony with prestate hash for supersession tracking
    #[allow(clippy::too_many_arguments)]
    pub async fn register_with_prestate(
        &self,
        ceremony_id: String,
        kind: CeremonyKind,
        threshold_k: u16,
        total_n: u16,
        participants: Vec<ParticipantIdentity>,
        new_epoch: u64,
        enrollment_device_id: Option<DeviceId>,
        prestate_hash: Option<Hash32>,
    ) -> Result<(), IntentError> {
        let state = CeremonyState {
            kind,
            threshold_k,
            total_n,
            participants,
            accepted_participants: Vec::new(),
            new_epoch,
            enrollment_device_id,
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
    pub async fn get(&self, ceremony_id: &str) -> Result<CeremonyState, IntentError> {
        let state = self.state.read().await;

        state.ceremonies.get(ceremony_id).cloned().ok_or_else(|| {
            IntentError::validation_failed(format!("Ceremony {} not found", ceremony_id))
        })
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
        ceremony_id: &str,
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
                state.accepted_participants.push(participant.clone());

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

    /// Mark a ceremony as committed (key rotation activated).
    ///
    /// This is only called after threshold is reached and `commit_key_rotation` succeeds.
    pub async fn mark_committed(&self, ceremony_id: &str) -> Result<(), IntentError> {
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
    pub async fn is_complete(&self, ceremony_id: &str) -> Result<bool, IntentError> {
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
    pub async fn is_timed_out(&self, ceremony_id: &str) -> Result<bool, IntentError> {
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
        ceremony_id: &str,
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
    pub async fn remove(&self, ceremony_id: &str) -> Result<(), IntentError> {
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
    pub async fn list_active(&self) -> Vec<(String, CeremonyState)> {
        let state = self.state.read().await;
        state
            .ceremonies
            .iter()
            .map(|(id, state)| (id.clone(), state.clone()))
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
        old_ceremony_id: &str,
        new_ceremony_id: &str,
        reason: SupersessionReason,
        timestamp_ms: u64,
    ) -> Result<SupersessionRecord, IntentError> {
        // Create record outside the lock scope
        let old_ceremony_hash = Hash32::from_bytes(old_ceremony_id.as_bytes());
        let new_ceremony_hash = Hash32::from_bytes(new_ceremony_id.as_bytes());
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
                old_state.superseded_by = Some(new_ceremony_id.to_string());
                old_state.has_failed = true;
                old_state.error_message = Some(format!("Superseded: {}", reason.description()));

                // Update new ceremony if it exists (may be registered separately)
                if let Some(new_state) = tracker.ceremonies.get_mut(new_ceremony_id) {
                    new_state.supersedes.push(old_ceremony_id.to_string());
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
    ) -> Vec<String> {
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
    pub async fn get_supersession_chain(&self, ceremony_id: &str) -> Vec<SupersessionRecord> {
        let ceremony_hash = Hash32::from_bytes(ceremony_id.as_bytes());
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
    pub async fn is_superseded(&self, ceremony_id: &str) -> Result<bool, IntentError> {
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
    use aura_core::identifiers::AuthorityId;
    use aura_core::DeviceId;

    #[tokio::test]
    async fn test_ceremony_registration() {
        let tracker = CeremonyTracker::new();

        let a = AuthorityId::new_from_entropy([1u8; 32]);
        let b = AuthorityId::new_from_entropy([2u8; 32]);
        let c = AuthorityId::new_from_entropy([3u8; 32]);

        tracker
            .register(
                "ceremony-1".to_string(),
                CeremonyKind::GuardianRotation,
                2,
                3,
                vec![
                    ParticipantIdentity::guardian(a),
                    ParticipantIdentity::guardian(b),
                    ParticipantIdentity::guardian(c),
                ],
                100,
                None,
            )
            .await
            .unwrap();

        let state = tracker.get("ceremony-1").await.unwrap();
        assert_eq!(state.threshold_k, 2);
        assert_eq!(state.total_n, 3);
        assert_eq!(state.participants.len(), 3);
        assert_eq!(state.accepted_participants.len(), 0);
    }

    #[tokio::test]
    async fn test_guardian_acceptance() {
        let tracker = CeremonyTracker::new();

        let a = AuthorityId::new_from_entropy([1u8; 32]);
        let b = AuthorityId::new_from_entropy([2u8; 32]);
        let c = AuthorityId::new_from_entropy([3u8; 32]);

        tracker
            .register(
                "ceremony-1".to_string(),
                CeremonyKind::GuardianRotation,
                2,
                3,
                vec![
                    ParticipantIdentity::guardian(a),
                    ParticipantIdentity::guardian(b),
                    ParticipantIdentity::guardian(c),
                ],
                100,
                None,
            )
            .await
            .unwrap();

        // First acceptance
        let threshold_reached = tracker
            .mark_accepted("ceremony-1", ParticipantIdentity::guardian(a))
            .await
            .unwrap();
        assert!(!threshold_reached);

        // Second acceptance - threshold reached
        let threshold_reached = tracker
            .mark_accepted("ceremony-1", ParticipantIdentity::guardian(b))
            .await
            .unwrap();
        assert!(threshold_reached);

        let state = tracker.get("ceremony-1").await.unwrap();
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

        let a = AuthorityId::new_from_entropy([1u8; 32]);
        let b = AuthorityId::new_from_entropy([2u8; 32]);
        let c = AuthorityId::new_from_entropy([3u8; 32]);

        tracker
            .register(
                "ceremony-1".to_string(),
                CeremonyKind::GuardianRotation,
                2,
                3,
                vec![
                    ParticipantIdentity::guardian(a),
                    ParticipantIdentity::guardian(b),
                    ParticipantIdentity::guardian(c),
                ],
                100,
                None,
            )
            .await
            .unwrap();

        assert!(!tracker.is_complete("ceremony-1").await.unwrap());

        tracker
            .mark_accepted("ceremony-1", ParticipantIdentity::guardian(a))
            .await
            .unwrap();
        assert!(!tracker.is_complete("ceremony-1").await.unwrap());

        let threshold_reached = tracker
            .mark_accepted("ceremony-1", ParticipantIdentity::guardian(b))
            .await
            .unwrap();
        assert!(threshold_reached);

        // Completion is only true once the key rotation is committed.
        assert!(!tracker.is_complete("ceremony-1").await.unwrap());
        tracker.mark_committed("ceremony-1").await.unwrap();
        assert!(tracker.is_complete("ceremony-1").await.unwrap());
    }

    #[tokio::test]
    async fn test_agreement_mode_transitions() {
        let tracker = CeremonyTracker::new();

        let a = AuthorityId::new_from_entropy([1u8; 32]);
        let b = AuthorityId::new_from_entropy([2u8; 32]);

        tracker
            .register(
                "ceremony-1".to_string(),
                CeremonyKind::GuardianRotation,
                2,
                2,
                vec![
                    ParticipantIdentity::guardian(a),
                    ParticipantIdentity::guardian(b),
                ],
                100,
                None,
            )
            .await
            .unwrap();

        let state = tracker.get("ceremony-1").await.unwrap();
        assert_eq!(state.agreement_mode, AgreementMode::CoordinatorSoftSafe);

        tracker
            .mark_accepted("ceremony-1", ParticipantIdentity::guardian(a))
            .await
            .unwrap();
        tracker
            .mark_accepted("ceremony-1", ParticipantIdentity::guardian(b))
            .await
            .unwrap();

        let state = tracker.get("ceremony-1").await.unwrap();
        assert_eq!(state.agreement_mode, AgreementMode::CoordinatorSoftSafe);

        tracker.mark_committed("ceremony-1").await.unwrap();
        let state = tracker.get("ceremony-1").await.unwrap();
        assert_eq!(state.agreement_mode, AgreementMode::ConsensusFinalized);
    }

    #[tokio::test]
    async fn test_idempotent_acceptance() {
        let tracker = CeremonyTracker::new();

        let a = AuthorityId::new_from_entropy([1u8; 32]);
        let b = AuthorityId::new_from_entropy([2u8; 32]);
        let c = AuthorityId::new_from_entropy([3u8; 32]);

        tracker
            .register(
                "ceremony-1".to_string(),
                CeremonyKind::GuardianRotation,
                2,
                3,
                vec![
                    ParticipantIdentity::guardian(a),
                    ParticipantIdentity::guardian(b),
                    ParticipantIdentity::guardian(c),
                ],
                100,
                None,
            )
            .await
            .unwrap();

        // Accept twice
        tracker
            .mark_accepted("ceremony-1", ParticipantIdentity::guardian(a))
            .await
            .unwrap();
        tracker
            .mark_accepted("ceremony-1", ParticipantIdentity::guardian(a))
            .await
            .unwrap();

        let state = tracker.get("ceremony-1").await.unwrap();
        assert_eq!(state.accepted_participants.len(), 1);
    }

    #[tokio::test]
    async fn test_ceremony_failure() {
        let tracker = CeremonyTracker::new();

        let a = AuthorityId::new_from_entropy([1u8; 32]);
        let b = AuthorityId::new_from_entropy([2u8; 32]);
        let c = AuthorityId::new_from_entropy([3u8; 32]);

        tracker
            .register(
                "ceremony-1".to_string(),
                CeremonyKind::GuardianRotation,
                2,
                3,
                vec![
                    ParticipantIdentity::guardian(a),
                    ParticipantIdentity::guardian(b),
                    ParticipantIdentity::guardian(c),
                ],
                100,
                None,
            )
            .await
            .unwrap();

        tracker
            .mark_failed("ceremony-1", Some("Test failure".to_string()))
            .await
            .unwrap();

        let state = tracker.get("ceremony-1").await.unwrap();
        assert!(state.has_failed);
        assert_eq!(state.error_message, Some("Test failure".to_string()));
    }

    #[tokio::test]
    async fn test_device_enrollment_ceremony_acceptance() {
        let tracker = CeremonyTracker::new();
        let device = DeviceId::new_from_entropy([9u8; 32]);

        tracker
            .register(
                "ceremony-device-1".to_string(),
                CeremonyKind::DeviceEnrollment,
                1,
                1,
                vec![ParticipantIdentity::device(device)],
                42,
                Some(device),
            )
            .await
            .unwrap();

        let state = tracker.get("ceremony-device-1").await.unwrap();
        assert_eq!(state.kind, CeremonyKind::DeviceEnrollment);
        assert_eq!(state.threshold_k, 1);
        assert_eq!(state.total_n, 1);

        let threshold_reached = tracker
            .mark_accepted("ceremony-device-1", ParticipantIdentity::device(device))
            .await
            .unwrap();
        assert!(threshold_reached);

        tracker.mark_committed("ceremony-device-1").await.unwrap();
        assert!(tracker.is_complete("ceremony-device-1").await.unwrap());
    }

    #[tokio::test]
    async fn test_device_rotation_agreement_mode_transitions() {
        let tracker = CeremonyTracker::new();
        let device_a = DeviceId::new_from_entropy([10u8; 32]);
        let device_b = DeviceId::new_from_entropy([11u8; 32]);

        tracker
            .register(
                "ceremony-rotate-1".to_string(),
                CeremonyKind::DeviceRotation,
                2,
                2,
                vec![
                    ParticipantIdentity::device(device_a),
                    ParticipantIdentity::device(device_b),
                ],
                77,
                None,
            )
            .await
            .unwrap();

        let state = tracker.get("ceremony-rotate-1").await.unwrap();
        assert_eq!(state.agreement_mode, AgreementMode::CoordinatorSoftSafe);

        tracker
            .mark_accepted("ceremony-rotate-1", ParticipantIdentity::device(device_a))
            .await
            .unwrap();
        tracker
            .mark_accepted("ceremony-rotate-1", ParticipantIdentity::device(device_b))
            .await
            .unwrap();

        tracker.mark_committed("ceremony-rotate-1").await.unwrap();
        let state = tracker.get("ceremony-rotate-1").await.unwrap();
        assert_eq!(state.agreement_mode, AgreementMode::ConsensusFinalized);
    }

    #[tokio::test]
    async fn test_device_removal_agreement_mode_transitions() {
        let tracker = CeremonyTracker::new();
        let device_a = DeviceId::new_from_entropy([12u8; 32]);
        let device_b = DeviceId::new_from_entropy([13u8; 32]);

        tracker
            .register(
                "ceremony-remove-1".to_string(),
                CeremonyKind::DeviceRemoval,
                2,
                2,
                vec![
                    ParticipantIdentity::device(device_a),
                    ParticipantIdentity::device(device_b),
                ],
                88,
                Some(device_b),
            )
            .await
            .unwrap();

        let state = tracker.get("ceremony-remove-1").await.unwrap();
        assert_eq!(state.agreement_mode, AgreementMode::CoordinatorSoftSafe);

        tracker
            .mark_accepted("ceremony-remove-1", ParticipantIdentity::device(device_a))
            .await
            .unwrap();
        tracker
            .mark_accepted("ceremony-remove-1", ParticipantIdentity::device(device_b))
            .await
            .unwrap();

        tracker.mark_committed("ceremony-remove-1").await.unwrap();
        let state = tracker.get("ceremony-remove-1").await.unwrap();
        assert_eq!(state.agreement_mode, AgreementMode::ConsensusFinalized);
    }
}
