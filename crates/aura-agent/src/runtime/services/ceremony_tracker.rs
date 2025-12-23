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

use aura_app::core::IntentError;
use aura_app::runtime_bridge::CeremonyKind;
use aura_core::threshold::ParticipantIdentity;
use aura_core::DeviceId;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{Duration, Instant};

/// Tracks state of guardian ceremonies
#[derive(Clone)]
pub struct CeremonyTracker {
    ceremonies: Arc<RwLock<HashMap<String, CeremonyState>>>,
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

    /// Optional error message if failed
    pub error_message: Option<String>,

    /// Timeout duration (30 seconds default)
    pub timeout: Duration,
}

impl CeremonyTracker {
    /// Create a new ceremony tracker
    pub fn new() -> Self {
        Self {
            ceremonies: Arc::new(RwLock::new(HashMap::new())),
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
        let mut ceremonies = self.ceremonies.write().await;

        if ceremonies.contains_key(&ceremony_id) {
            return Err(IntentError::validation_failed(format!(
                "Ceremony {} already registered",
                ceremony_id
            )));
        }

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
            error_message: None,
            timeout: Duration::from_secs(30),
        };

        ceremonies.insert(ceremony_id.clone(), state);

        tracing::info!(
            ceremony_id = %ceremony_id,
            threshold_k,
            total_n,
            "Ceremony registered"
        );

        Ok(())
    }

    /// Get ceremony state
    ///
    /// # Arguments
    /// * `ceremony_id` - The ceremony identifier
    ///
    /// # Returns
    /// The current ceremony state
    pub async fn get(&self, ceremony_id: &str) -> Result<CeremonyState, IntentError> {
        let ceremonies = self.ceremonies.read().await;

        ceremonies.get(ceremony_id).cloned().ok_or_else(|| {
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
        let mut ceremonies = self.ceremonies.write().await;

        let state = ceremonies.get_mut(ceremony_id).ok_or_else(|| {
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

        let threshold_reached = state.accepted_participants.len() >= state.threshold_k as usize;

        tracing::info!(
            ceremony_id = %ceremony_id,
            accepted = state.accepted_participants.len(),
            threshold = state.threshold_k,
            threshold_reached,
            "Participant accepted ceremony"
        );

        Ok(threshold_reached)
    }

    /// Mark a ceremony as committed (key rotation activated).
    ///
    /// This is only called after threshold is reached and `commit_key_rotation` succeeds.
    pub async fn mark_committed(&self, ceremony_id: &str) -> Result<(), IntentError> {
        let mut ceremonies = self.ceremonies.write().await;

        let state = ceremonies.get_mut(ceremony_id).ok_or_else(|| {
            IntentError::validation_failed(format!("Ceremony {} not found", ceremony_id))
        })?;

        if state.is_committed {
            return Ok(());
        }

        state.is_committed = true;

        tracing::info!(
            ceremony_id = %ceremony_id,
            accepted = state.accepted_participants.len(),
            threshold = state.threshold_k,
            "Ceremony committed"
        );

        Ok(())
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
        let mut ceremonies = self.ceremonies.write().await;

        let state = ceremonies.get_mut(ceremony_id).ok_or_else(|| {
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
    }

    /// Remove ceremony from tracker (cleanup after completion/failure)
    ///
    /// # Arguments
    /// * `ceremony_id` - The ceremony identifier
    pub async fn remove(&self, ceremony_id: &str) -> Result<(), IntentError> {
        let mut ceremonies = self.ceremonies.write().await;

        ceremonies.remove(ceremony_id).ok_or_else(|| {
            IntentError::validation_failed(format!("Ceremony {} not found", ceremony_id))
        })?;

        tracing::debug!(ceremony_id = %ceremony_id, "Ceremony removed from tracker");

        Ok(())
    }

    /// Get list of all active ceremonies
    ///
    /// # Returns
    /// Vector of (ceremony_id, state) tuples
    pub async fn list_active(&self) -> Vec<(String, CeremonyState)> {
        let ceremonies = self.ceremonies.read().await;
        ceremonies
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
        let mut ceremonies = self.ceremonies.write().await;
        let mut removed = Vec::new();

        for (id, state) in ceremonies.iter() {
            if state.started_at.elapsed() > state.timeout && !state.has_failed {
                removed.push(id.clone());
            }
        }

        for id in &removed {
            if let Some(state) = ceremonies.get_mut(id) {
                state.has_failed = true;
                state.error_message = Some("Ceremony timed out".to_string());
            }
            tracing::warn!(ceremony_id = %id, "Ceremony timed out and marked as failed");
        }

        removed.len()
    }
}

impl Default for CeremonyTracker {
    fn default() -> Self {
        Self::new()
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
}
