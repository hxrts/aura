//! Ledger persistence for recovery state and evidence.
//!
//! Provides durable storage of recovery evidence, disputes, and state for audit trails.

use crate::types::{RecoveryDispute, RecoveryEvidence};
use aura_core::{
    serialization::{from_slice, to_vec},
    AccountId, AuraError, AuraResult, DeviceId,
};
use aura_protocol::effects::{AuraEffectSystem, StorageEffects};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Recovery state ledger for persistent audit trail
#[derive(Debug, Clone)]
pub struct RecoveryLedger {
    effects: AuraEffectSystem,
}

/// Recovery session state for persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoverySessionState {
    /// Account being recovered
    pub account_id: AccountId,
    /// Device requesting recovery
    pub requesting_device: DeviceId,
    /// Recovery evidence
    pub evidence: RecoveryEvidence,
    /// Session status
    pub status: RecoverySessionStatus,
    /// Created timestamp
    pub created_at: u64,
    /// Last updated timestamp
    pub updated_at: u64,
}

/// Recovery session status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RecoverySessionStatus {
    /// Pending guardian approvals
    Pending,
    /// In dispute window
    InDisputeWindow,
    /// Completed successfully
    Completed,
    /// Cancelled due to disputes or policy
    Cancelled { reason: String },
    /// Failed with error
    Failed { error: String },
}

impl RecoveryLedger {
    /// Create new recovery ledger
    pub fn new(effects: AuraEffectSystem) -> Self {
        Self { effects }
    }

    /// Store recovery evidence in ledger
    pub async fn store_evidence(
        &self,
        account_id: &AccountId,
        evidence: &RecoveryEvidence,
    ) -> AuraResult<()> {
        let key = evidence_key(account_id, evidence.issued_at);
        let bytes = to_vec(evidence)
            .map_err(|e| AuraError::serialization_failed(format!("encode evidence: {}", e)))?;

        StorageEffects::store(&self.effects, &key, bytes)
            .await
            .map_err(|e| AuraError::storage(format!("store evidence: {}", e)))
    }

    /// Retrieve recovery evidence
    pub async fn get_evidence(
        &self,
        account_id: &AccountId,
        timestamp: u64,
    ) -> AuraResult<Option<RecoveryEvidence>> {
        let key = evidence_key(account_id, timestamp);

        match StorageEffects::retrieve(&self.effects, &key).await {
            Ok(Some(bytes)) => {
                let evidence = from_slice(&bytes).map_err(|e| {
                    AuraError::deserialization_failed(format!("decode evidence: {}", e))
                })?;
                Ok(Some(evidence))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(AuraError::storage(format!("retrieve evidence: {}", e))),
        }
    }

    /// List all recovery evidence for an account
    pub async fn list_evidence(&self, account_id: &AccountId) -> AuraResult<Vec<RecoveryEvidence>> {
        let prefix = format!("recovery:evidence:{}:", account_id);
        let keys = StorageEffects::list_keys(&self.effects, Some(&prefix))
            .await
            .unwrap_or_default();

        let mut evidence_list = Vec::new();
        for key in keys {
            if let Ok(Some(bytes)) = StorageEffects::retrieve(&self.effects, &key).await {
                if let Ok(evidence) = from_slice::<RecoveryEvidence>(&bytes) {
                    evidence_list.push(evidence);
                }
            }
        }

        // Sort by timestamp (most recent first)
        evidence_list.sort_by(|a, b| b.issued_at.cmp(&a.issued_at));
        Ok(evidence_list)
    }

    /// Store recovery session state
    pub async fn store_session_state(&self, session: &RecoverySessionState) -> AuraResult<()> {
        let key = session_state_key(&session.account_id, &session.requesting_device);
        let bytes = to_vec(session)
            .map_err(|e| AuraError::serialization_failed(format!("encode session: {}", e)))?;

        StorageEffects::store(&self.effects, &key, bytes)
            .await
            .map_err(|e| AuraError::storage(format!("store session: {}", e)))
    }

    /// Get recovery session state
    pub async fn get_session_state(
        &self,
        account_id: &AccountId,
        device_id: &DeviceId,
    ) -> AuraResult<Option<RecoverySessionState>> {
        let key = session_state_key(account_id, device_id);

        match StorageEffects::retrieve(&self.effects, &key).await {
            Ok(Some(bytes)) => {
                let session = from_slice(&bytes).map_err(|e| {
                    AuraError::deserialization_failed(format!("decode session: {}", e))
                })?;
                Ok(Some(session))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(AuraError::storage(format!("retrieve session: {}", e))),
        }
    }

    /// Add dispute to recovery evidence
    pub async fn add_dispute(
        &self,
        account_id: &AccountId,
        evidence_timestamp: u64,
        dispute: RecoveryDispute,
    ) -> AuraResult<()> {
        // Retrieve existing evidence
        let mut evidence = self
            .get_evidence(account_id, evidence_timestamp)
            .await?
            .ok_or_else(|| AuraError::not_found("Recovery evidence not found"))?;

        // Add dispute
        evidence.disputes.push(dispute);

        // Store updated evidence
        self.store_evidence(account_id, &evidence).await
    }

    /// Get dispute count for account
    pub async fn get_dispute_count(&self, account_id: &AccountId) -> AuraResult<usize> {
        let evidence_list = self.list_evidence(account_id).await?;
        let total_disputes: usize = evidence_list.iter().map(|e| e.disputes.len()).sum();
        Ok(total_disputes)
    }

    /// Get active recovery sessions for account
    pub async fn get_active_sessions(
        &self,
        account_id: &AccountId,
    ) -> AuraResult<Vec<RecoverySessionState>> {
        let prefix = format!("recovery:session:{}:", account_id);
        let keys = StorageEffects::list_keys(&self.effects, Some(&prefix))
            .await
            .unwrap_or_default();

        let mut sessions = Vec::new();
        for key in keys {
            if let Ok(Some(bytes)) = StorageEffects::retrieve(&self.effects, &key).await {
                if let Ok(session) = from_slice::<RecoverySessionState>(&bytes) {
                    // Only include active sessions
                    if matches!(
                        session.status,
                        RecoverySessionStatus::Pending | RecoverySessionStatus::InDisputeWindow
                    ) {
                        sessions.push(session);
                    }
                }
            }
        }

        Ok(sessions)
    }

    /// Cleanup old completed/failed sessions
    pub async fn cleanup_old_sessions(
        &self,
        account_id: &AccountId,
        older_than: u64,
    ) -> AuraResult<usize> {
        let prefix = format!("recovery:session:{}:", account_id);
        let keys = StorageEffects::list_keys(&self.effects, Some(&prefix))
            .await
            .unwrap_or_default();

        let mut cleaned = 0;
        for key in keys {
            if let Ok(Some(bytes)) = StorageEffects::retrieve(&self.effects, &key).await {
                if let Ok(session) = from_slice::<RecoverySessionState>(&bytes) {
                    // Remove completed/failed sessions older than threshold
                    let should_cleanup = session.updated_at < older_than
                        && !matches!(
                            session.status,
                            RecoverySessionStatus::Pending | RecoverySessionStatus::InDisputeWindow
                        );

                    if should_cleanup {
                        if StorageEffects::remove(&self.effects, &key).await.is_ok() {
                            cleaned += 1;
                        }
                    }
                }
            }
        }

        Ok(cleaned)
    }
}

/// Generate storage key for recovery evidence
fn evidence_key(account_id: &AccountId, timestamp: u64) -> String {
    format!("recovery:evidence:{}:{}", account_id, timestamp)
}

/// Generate storage key for recovery session state
fn session_state_key(account_id: &AccountId, device_id: &DeviceId) -> String {
    format!("recovery:session:{}:{}", account_id, device_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::identifiers::GuardianId;

    #[test]
    fn test_key_generation() {
        let account_id = AccountId::new();
        let device_id = DeviceId::new();
        let timestamp = 1234567890;

        let evidence_key = evidence_key(&account_id, timestamp);
        assert!(evidence_key.starts_with("recovery:evidence:"));
        assert!(evidence_key.contains(&timestamp.to_string()));

        let session_key = session_state_key(&account_id, &device_id);
        assert!(session_key.starts_with("recovery:session:"));
    }

    #[test]
    fn test_session_status() {
        let status = RecoverySessionStatus::Pending;
        assert_eq!(status, RecoverySessionStatus::Pending);

        let cancelled = RecoverySessionStatus::Cancelled {
            reason: "Too many disputes".to_string(),
        };
        assert!(matches!(cancelled, RecoverySessionStatus::Cancelled { .. }));
    }
}
