//! Recovery cache manager.

use super::state::with_state_mut_validated;
use crate::handlers::recovery::{ActiveRecovery, RecoveryState};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Default)]
struct RecoveryStateCache {
    recoveries: HashMap<String, ActiveRecovery>,
}

impl RecoveryStateCache {
    fn validate(&self) -> Result<(), String> {
        for (id, recovery) in &self.recoveries {
            if id != &recovery.request.recovery_id {
                return Err(format!(
                    "recovery id mismatch: key {} vs value {}",
                    id, recovery.request.recovery_id
                ));
            }
        }
        Ok(())
    }
}

/// Manages active recovery ceremonies for the recovery handler.
#[derive(Clone, Default)]
pub struct RecoveryManager {
    state: Arc<RwLock<RecoveryStateCache>>,
}

impl RecoveryManager {
    /// Create a new recovery manager.
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(RecoveryStateCache::default())),
        }
    }

    /// Insert a new active recovery.
    pub async fn insert(&self, recovery_id: String, recovery: ActiveRecovery) {
        with_state_mut_validated(
            &self.state,
            |state| {
                state.recoveries.insert(recovery_id, recovery);
            },
            |state| state.validate(),
        )
        .await;
    }

    /// Get the state of a recovery.
    pub async fn get_state(&self, recovery_id: &str) -> Option<RecoveryState> {
        self.state
            .read()
            .await
            .recoveries
            .get(recovery_id)
            .map(|r| r.state.clone())
    }

    /// Get a cloned recovery.
    pub async fn get_recovery(&self, recovery_id: &str) -> Option<ActiveRecovery> {
        self.state.read().await.recoveries.get(recovery_id).cloned()
    }

    /// Mutate a recovery if present.
    pub async fn with_recovery_mut<R>(
        &self,
        recovery_id: &str,
        f: impl FnOnce(&mut ActiveRecovery) -> R,
    ) -> Option<R> {
        with_state_mut_validated(
            &self.state,
            |state| state.recoveries.get_mut(recovery_id).map(f),
            |state| state.validate(),
        )
        .await
    }

    /// Remove a recovery.
    pub async fn remove(&self, recovery_id: &str) -> Option<ActiveRecovery> {
        with_state_mut_validated(
            &self.state,
            |state| state.recoveries.remove(recovery_id),
            |state| state.validate(),
        )
        .await
    }

    /// List active recoveries as (id, state).
    pub async fn list_active(&self) -> Vec<(String, RecoveryState)> {
        self.state
            .read()
            .await
            .recoveries
            .iter()
            .map(|(id, recovery)| (id.clone(), recovery.state.clone()))
            .collect()
    }

    /// Cleanup expired recoveries. Returns removed count.
    pub async fn cleanup_expired(&self, current_time: u64) -> usize {
        with_state_mut_validated(
            &self.state,
            |state| {
                let before = state.recoveries.len();
                state
                    .recoveries
                    .retain(|_, r| r.request.expires_at.map_or(true, |exp| exp > current_time));
                before - state.recoveries.len()
            },
            |state| state.validate(),
        )
        .await
    }
}
