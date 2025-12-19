//! Recovery Workflow - Portable Business Logic
//!
//! This module contains guardian recovery operations that are portable
//! across all frontends. It follows the reactive signal pattern and
//! uses RuntimeBridge for runtime operations.

use crate::{
    runtime_bridge::CeremonyStatus,
    views::recovery::{RecoveryProcess, RecoveryProcessStatus, RecoveryState},
    AppCore, RECOVERY_SIGNAL,
};
use async_lock::RwLock;
use aura_core::{effects::reactive::ReactiveEffects, identifiers::AuthorityId, AuraError};
use std::sync::Arc;

/// Start a guardian recovery ceremony
///
/// **What it does**: Initiates guardian key rotation ceremony
/// **Returns**: Ceremony ID for tracking progress
/// **Signal pattern**: Emits RECOVERY_SIGNAL after initiation
///
/// This operation:
/// 1. Generates new FROST threshold keys for guardians
/// 2. Sends guardian invitations with key packages
/// 3. Waits for guardian acceptances
/// 4. Emits recovery state signal for UI updates
///
/// The ceremony is non-blocking - guardians can respond asynchronously.
pub async fn start_recovery(
    app_core: &Arc<RwLock<AppCore>>,
    guardian_ids: Vec<String>,
    threshold_k: u16,
) -> Result<String, AuraError> {
    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .ok_or_else(|| AuraError::agent("Runtime bridge not available"))?
            .clone()
    };

    // Initiate guardian ceremony via runtime bridge
    let total_n = guardian_ids.len() as u16;
    let ceremony_id = runtime
        .initiate_guardian_ceremony(threshold_k, total_n, &guardian_ids)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to start recovery: {}", e)))?;

    // Emit recovery signal with initial state
    let recovery_process = RecoveryProcess {
        id: ceremony_id.clone(),
        account_id: AuthorityId::default(), // Would be set from context
        status: RecoveryProcessStatus::WaitingForApprovals,
        approvals_received: 0,
        approvals_required: threshold_k as u32,
        approved_by: Vec::new(),
        approvals: Vec::new(),
        initiated_at: 0, // Would be set from PhysicalTimeEffects
        expires_at: None,
        progress: 0,
    };

    let state = RecoveryState {
        guardians: Vec::new(), // Would be populated from guardian list
        threshold: threshold_k as u32,
        guardian_count: guardian_ids.len() as u32,
        active_recovery: Some(recovery_process),
        pending_requests: Vec::new(),
    };

    emit_recovery_signal(app_core, state).await?;

    Ok(ceremony_id)
}

/// Approve a recovery request as a guardian
///
/// **What it does**: Records guardian approval for recovery ceremony
/// **Returns**: Unit result
/// **Signal pattern**: RuntimeBridge handles signal emission
pub async fn approve_recovery(
    app_core: &Arc<RwLock<AppCore>>,
    ceremony_id: &str,
) -> Result<(), AuraError> {
    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .ok_or_else(|| AuraError::agent("Runtime bridge not available"))?
            .clone()
    };

    runtime
        .respond_to_guardian_ceremony(ceremony_id, true, None)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to approve recovery: {}", e)))
}

/// Dispute a recovery request
///
/// **What it does**: Files a dispute against a recovery ceremony
/// **Returns**: Unit result
/// **Signal pattern**: RuntimeBridge handles signal emission
pub async fn dispute_recovery(
    app_core: &Arc<RwLock<AppCore>>,
    ceremony_id: &str,
    reason: String,
) -> Result<(), AuraError> {
    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .ok_or_else(|| AuraError::agent("Runtime bridge not available"))?
            .clone()
    };

    runtime
        .respond_to_guardian_ceremony(ceremony_id, false, Some(reason))
        .await
        .map_err(|e| AuraError::agent(format!("Failed to dispute recovery: {}", e)))
}

/// Get current recovery status
///
/// **What it does**: Reads recovery state from RECOVERY_SIGNAL
/// **Returns**: Current recovery state
/// **Signal pattern**: Read-only operation (no emission)
pub async fn get_recovery_status(app_core: &Arc<RwLock<AppCore>>) -> RecoveryState {
    let core = app_core.read().await;

    core.read(&*RECOVERY_SIGNAL).await.unwrap_or_default()
}

/// Get ceremony status from runtime
///
/// **What it does**: Queries runtime bridge for ceremony progress
/// **Returns**: Ceremony status with approval counts
/// **Signal pattern**: Read-only operation (no emission)
///
/// Use this to poll ceremony progress. The runtime tracks guardian
/// approvals and ceremony completion state.
pub async fn get_ceremony_status(
    app_core: &Arc<RwLock<AppCore>>,
    ceremony_id: &str,
) -> Result<CeremonyStatus, AuraError> {
    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .ok_or_else(|| AuraError::agent("Runtime bridge not available"))?
            .clone()
    };

    runtime
        .get_ceremony_status(ceremony_id)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to get ceremony status: {}", e)))
}

/// Emit recovery signal with updated state
async fn emit_recovery_signal(
    app_core: &Arc<RwLock<AppCore>>,
    state: RecoveryState,
) -> Result<(), AuraError> {
    let core = app_core.read().await;
    core.emit(&*RECOVERY_SIGNAL, state)
        .await
        .map_err(|e| AuraError::internal(format!("Failed to emit recovery signal: {}", e)))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::recovery::{
        Guardian, GuardianStatus, RecoveryProcess, RecoveryProcessStatus,
    };
    use crate::AppConfig;
    use aura_core::identifiers::AuthorityId;

    #[tokio::test]
    async fn test_get_recovery_status_default() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let status = get_recovery_status(&app_core).await;
        assert!(status.active_recovery.is_none());
        assert!(status.guardians.is_empty());
    }

    #[tokio::test]
    async fn test_emit_recovery_signal() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        // Register signal
        {
            let core = app_core.read().await;
            core.register(&*RECOVERY_SIGNAL, RecoveryState::default())
                .await
                .unwrap();
        }

        // Emit recovery state with active recovery process
        let state = RecoveryState {
            guardians: vec![Guardian {
                id: AuthorityId::default(),
                name: "Alice".to_string(),
                status: GuardianStatus::Active,
                added_at: 1000,
                last_seen: Some(2000),
            }],
            threshold: 2,
            guardian_count: 3,
            active_recovery: Some(RecoveryProcess {
                id: "ceremony-123".to_string(),
                account_id: AuthorityId::default(),
                status: RecoveryProcessStatus::WaitingForApprovals,
                approvals_received: 0,
                approvals_required: 2,
                approved_by: vec![],
                approvals: vec![],
                initiated_at: 1000,
                expires_at: Some(2000),
                progress: 0,
            }),
            pending_requests: vec![],
        };

        emit_recovery_signal(&app_core, state.clone())
            .await
            .unwrap();

        // Verify state was emitted
        let retrieved = get_recovery_status(&app_core).await;
        assert!(retrieved.active_recovery.is_some());
        assert_eq!(
            retrieved.active_recovery.as_ref().unwrap().id,
            "ceremony-123"
        );
    }
}
