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
    /// Threshold required for completion (k)
    pub threshold_k: u16,

    /// Total number of guardians (n)
    pub total_n: u16,

    /// Guardian IDs invited to participate
    pub guardian_ids: Vec<String>,

    /// Guardian IDs who have accepted
    pub accepted_guardians: Vec<String>,

    /// New epoch for the key rotation
    pub new_epoch: u64,

    /// When the ceremony was initiated
    pub started_at: Instant,

    /// Whether the ceremony has failed
    pub has_failed: bool,

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
    /// * `total_n` - Total number of guardians
    /// * `guardian_ids` - IDs of guardians invited
    /// * `new_epoch` - Epoch for the new keys
    pub async fn register(
        &self,
        ceremony_id: String,
        threshold_k: u16,
        total_n: u16,
        guardian_ids: Vec<String>,
        new_epoch: u64,
    ) -> Result<(), IntentError> {
        let mut ceremonies = self.ceremonies.write().await;

        if ceremonies.contains_key(&ceremony_id) {
            return Err(IntentError::validation_failed(format!(
                "Ceremony {} already registered",
                ceremony_id
            )));
        }

        let state = CeremonyState {
            threshold_k,
            total_n,
            guardian_ids,
            accepted_guardians: Vec::new(),
            new_epoch,
            started_at: Instant::now(),
            has_failed: false,
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

        ceremonies
            .get(ceremony_id)
            .cloned()
            .ok_or_else(|| IntentError::validation_failed(format!("Ceremony {} not found", ceremony_id)))
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
        guardian_id: String,
    ) -> Result<bool, IntentError> {
        let mut ceremonies = self.ceremonies.write().await;

        let state = ceremonies
            .get_mut(ceremony_id)
            .ok_or_else(|| IntentError::validation_failed(format!("Ceremony {} not found", ceremony_id)))?;

        // Check if guardian is part of this ceremony
        if !state.guardian_ids.contains(&guardian_id) {
            return Err(IntentError::validation_failed(format!(
                "Guardian {} not part of ceremony {}",
                guardian_id, ceremony_id
            )));
        }

        // Check if already accepted
        if state.accepted_guardians.contains(&guardian_id) {
            tracing::debug!(
                ceremony_id = %ceremony_id,
                guardian_id = %guardian_id,
                "Guardian already accepted (idempotent)"
            );
            return Ok(state.accepted_guardians.len() >= state.threshold_k as usize);
        }

        // Add to accepted list
        state.accepted_guardians.push(guardian_id.clone());

        let threshold_reached = state.accepted_guardians.len() >= state.threshold_k as usize;

        tracing::info!(
            ceremony_id = %ceremony_id,
            guardian_id = %guardian_id,
            accepted = state.accepted_guardians.len(),
            threshold = state.threshold_k,
            threshold_reached,
            "Guardian accepted invitation"
        );

        Ok(threshold_reached)
    }

    /// Check if ceremony is complete (threshold reached)
    ///
    /// # Arguments
    /// * `ceremony_id` - The ceremony identifier
    ///
    /// # Returns
    /// True if threshold is reached
    pub async fn is_complete(&self, ceremony_id: &str) -> Result<bool, IntentError> {
        let state = self.get(ceremony_id).await?;
        Ok(state.accepted_guardians.len() >= state.threshold_k as usize)
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

        let state = ceremonies
            .get_mut(ceremony_id)
            .ok_or_else(|| IntentError::validation_failed(format!("Ceremony {} not found", ceremony_id)))?;

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

        ceremonies
            .remove(ceremony_id)
            .ok_or_else(|| IntentError::validation_failed(format!("Ceremony {} not found", ceremony_id)))?;

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

    #[tokio::test]
    async fn test_ceremony_registration() {
        let tracker = CeremonyTracker::new();

        tracker
            .register(
                "ceremony-1".to_string(),
                2,
                3,
                vec!["alice".to_string(), "bob".to_string(), "carol".to_string()],
                100,
            )
            .await
            .unwrap();

        let state = tracker.get("ceremony-1").await.unwrap();
        assert_eq!(state.threshold_k, 2);
        assert_eq!(state.total_n, 3);
        assert_eq!(state.guardian_ids.len(), 3);
        assert_eq!(state.accepted_guardians.len(), 0);
    }

    #[tokio::test]
    async fn test_guardian_acceptance() {
        let tracker = CeremonyTracker::new();

        tracker
            .register(
                "ceremony-1".to_string(),
                2,
                3,
                vec!["alice".to_string(), "bob".to_string(), "carol".to_string()],
                100,
            )
            .await
            .unwrap();

        // First acceptance
        let threshold_reached = tracker
            .mark_accepted("ceremony-1", "alice".to_string())
            .await
            .unwrap();
        assert!(!threshold_reached);

        // Second acceptance - threshold reached
        let threshold_reached = tracker
            .mark_accepted("ceremony-1", "bob".to_string())
            .await
            .unwrap();
        assert!(threshold_reached);

        let state = tracker.get("ceremony-1").await.unwrap();
        assert_eq!(state.accepted_guardians.len(), 2);
        assert!(state.accepted_guardians.contains(&"alice".to_string()));
        assert!(state.accepted_guardians.contains(&"bob".to_string()));
    }

    #[tokio::test]
    async fn test_ceremony_completion() {
        let tracker = CeremonyTracker::new();

        tracker
            .register(
                "ceremony-1".to_string(),
                2,
                3,
                vec!["alice".to_string(), "bob".to_string(), "carol".to_string()],
                100,
            )
            .await
            .unwrap();

        assert!(!tracker.is_complete("ceremony-1").await.unwrap());

        tracker
            .mark_accepted("ceremony-1", "alice".to_string())
            .await
            .unwrap();
        assert!(!tracker.is_complete("ceremony-1").await.unwrap());

        tracker
            .mark_accepted("ceremony-1", "bob".to_string())
            .await
            .unwrap();
        assert!(tracker.is_complete("ceremony-1").await.unwrap());
    }

    #[tokio::test]
    async fn test_idempotent_acceptance() {
        let tracker = CeremonyTracker::new();

        tracker
            .register(
                "ceremony-1".to_string(),
                2,
                3,
                vec!["alice".to_string(), "bob".to_string(), "carol".to_string()],
                100,
            )
            .await
            .unwrap();

        // Accept twice
        tracker
            .mark_accepted("ceremony-1", "alice".to_string())
            .await
            .unwrap();
        tracker
            .mark_accepted("ceremony-1", "alice".to_string())
            .await
            .unwrap();

        let state = tracker.get("ceremony-1").await.unwrap();
        assert_eq!(state.accepted_guardians.len(), 1);
    }

    #[tokio::test]
    async fn test_ceremony_failure() {
        let tracker = CeremonyTracker::new();

        tracker
            .register(
                "ceremony-1".to_string(),
                2,
                3,
                vec!["alice".to_string(), "bob".to_string(), "carol".to_string()],
                100,
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
}
