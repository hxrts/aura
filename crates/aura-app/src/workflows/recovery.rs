//! Recovery Workflow - Portable Business Logic
//!
//! This module contains guardian recovery operations that are portable
//! across all frontends. Uses typed reactive signals for state reads/writes.

use crate::{
    runtime_bridge::CeremonyStatus,
    views::recovery::{RecoveryProcess, RecoveryProcessStatus, RecoveryState},
    AppCore,
};
use async_lock::RwLock;
use aura_core::{
    identifiers::AuthorityId, types::FrostThreshold, AuraError, Hash32,
};
use aura_journal::fact::RelationalFact;
use crate::workflows::runtime::require_runtime;
use crate::workflows::parse::parse_authority_id;
use aura_journal::ProtocolRelationalFact;
use std::sync::Arc;
use std::future::Future;
use std::time::Duration;
use crate::workflows::ceremonies::{CeremonyPollPolicy, CeremonyLifecycle, CeremonyLifecycleState, CeremonyStatusLike};

/// Start a guardian recovery ceremony
///
/// **What it does**: Initiates guardian key rotation ceremony
/// **Returns**: Ceremony ID for tracking progress
/// **Signal pattern**: Updates ViewState; signal forwarding handles RECOVERY_SIGNAL
///
/// This operation:
/// 1. Generates new FROST threshold keys for guardians
/// 2. Sends guardian invitations with key packages
/// 3. Waits for guardian acceptances
/// 4. ViewState update auto-forwards to RECOVERY_SIGNAL for UI updates
///
/// The ceremony is asynchronous; guardians can respond over time.
pub async fn start_recovery(
    app_core: &Arc<RwLock<AppCore>>,
    guardian_ids: Vec<String>,
    threshold_k: FrostThreshold,
) -> Result<String, AuraError> {
    let runtime = {
        require_runtime(app_core).await?
    };

    if guardian_ids.is_empty() {
        return Err(AuraError::invalid("Guardian list cannot be empty"));
    }

    if guardian_ids.len() < threshold_k.value() as usize {
        return Err(AuraError::invalid(format!(
            "Threshold {} exceeds guardian count {}",
            threshold_k.value(),
            guardian_ids.len()
        )));
    }

    let initiated_at = runtime
        .current_time_ms()
        .await
        .map_err(|e| AuraError::agent(format!("Failed to get time: {e}")))?;

    let guardians = guardian_ids
        .iter()
        .map(|id| {
            parse_authority_id(id).map(|authority| crate::views::recovery::Guardian {
                id: authority,
                name: String::new(),
                status: crate::views::recovery::GuardianStatus::Pending,
                added_at: initiated_at,
                last_seen: None,
            })
        })
        .collect::<Result<Vec<_>, AuraError>>()?;

    // Initiate guardian ceremony via runtime bridge
    let total_n = guardian_ids.len() as u16;
    let ceremony_id = runtime
        .initiate_guardian_ceremony(threshold_k, total_n, &guardian_ids)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to start recovery: {}", e)))?;

    // Create recovery state with initial process
    let recovery_process = RecoveryProcess {
        id: ceremony_id.clone(),
        account_id: runtime.authority_id(),
        status: RecoveryProcessStatus::WaitingForApprovals,
        approvals_received: 0,
        approvals_required: threshold_k.value() as u32,
        approved_by: Vec::new(),
        approvals: Vec::new(),
        initiated_at,
        expires_at: None,
        progress: 0,
    };

    let state = RecoveryState {
        guardians,
        threshold: threshold_k.value() as u32,
        guardian_count: guardian_ids.len() as u32,
        active_recovery: Some(recovery_process),
        pending_requests: Vec::new(),
    };

    // Update ViewState - signal forwarding auto-propagates to RECOVERY_SIGNAL
    set_recovery_state(app_core, state).await?;

    Ok(ceremony_id)
}

/// Start recovery using the current recovery state in RECOVERY_SIGNAL.
///
/// **What it does**: Validates guardian set + threshold from state and starts ceremony.
/// **Signal pattern**: Updates RECOVERY_SIGNAL directly
pub async fn start_recovery_from_state(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<String, AuraError> {
    let state = get_recovery_status(app_core).await?;

    if state.guardians.is_empty() {
        return Err(AuraError::agent(
            "No guardians configured. Add guardians in Threshold settings first.",
        ));
    }

    if state.active_recovery.is_some() {
        return Err(AuraError::agent("Recovery already in progress"));
    }

    let guardian_ids: Vec<String> =
        state.guardians.iter().map(|g| g.id.to_string()).collect();

    let threshold = FrostThreshold::new(state.threshold as u16).map_err(|e| {
        AuraError::invalid(format!("Invalid recovery threshold {}: {e}", state.threshold))
    })?;

    start_recovery(app_core, guardian_ids, threshold).await
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
        require_runtime(app_core).await?
    };

    runtime
        .respond_to_guardian_ceremony(ceremony_id, true, None)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to approve recovery: {}", e)))
}

/// Commit a GuardianBinding fact via the runtime bridge (demo/testing helper).
pub async fn commit_guardian_binding(
    app_core: &Arc<RwLock<AppCore>>,
    account_id: AuthorityId,
    guardian_id: AuthorityId,
    binding_hash: Hash32,
) -> Result<(), AuraError> {
    let runtime = {
        require_runtime(app_core).await?
    };

    let fact = RelationalFact::Protocol(ProtocolRelationalFact::GuardianBinding {
        account_id,
        guardian_id,
        binding_hash,
    });

    runtime
        .commit_relational_facts(&[fact])
        .await
        .map_err(|e| AuraError::agent(format!("Failed to commit guardian binding: {}", e)))
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
        require_runtime(app_core).await?
    };

    runtime
        .respond_to_guardian_ceremony(ceremony_id, false, Some(reason))
        .await
        .map_err(|e| AuraError::agent(format!("Failed to dispute recovery: {}", e)))
}

/// Get current recovery status
///
/// **What it does**: Reads recovery state from ViewState
/// **Returns**: Current recovery state
/// **Signal pattern**: Read-only operation (no emission)
pub async fn get_recovery_status(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<RecoveryState, AuraError> {
    let core = app_core.read().await;
    Ok(core.snapshot().recovery)
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
        require_runtime(app_core).await?
    };

    runtime
        .get_ceremony_status(ceremony_id)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to get ceremony status: {}", e)))
}

impl CeremonyStatusLike for CeremonyStatus {
    fn is_complete(&self) -> bool {
        self.is_complete
    }

    fn has_failed(&self) -> bool {
        self.has_failed
    }
}

/// Poll a recovery ceremony until completion or failure using a policy.
pub async fn monitor_recovery_ceremony_with_policy<SleepFn, SleepFut>(
    app_core: &Arc<RwLock<AppCore>>,
    ceremony_id: String,
    policy: CeremonyPollPolicy,
    mut on_update: impl FnMut(&CeremonyStatus) + Send,
    mut sleep_fn: SleepFn,
) -> Result<CeremonyLifecycle<CeremonyStatus>, AuraError>
where
    SleepFn: FnMut(Duration) -> SleepFut + Send,
    SleepFut: Future<Output = ()> + Send,
{
    for attempt in 1..=policy.max_attempts {
        sleep_fn(policy.interval).await;

        let status = get_ceremony_status(app_core, &ceremony_id).await?;
        on_update(&status);

        if status.has_failed {
            return Ok(CeremonyLifecycle {
                state: CeremonyLifecycleState::Failed,
                status,
                attempts: attempt,
            });
        }

        if status.is_complete {
            return Ok(CeremonyLifecycle {
                state: CeremonyLifecycleState::Completed,
                status,
                attempts: attempt,
            });
        }
    }

    let status = get_ceremony_status(app_core, &ceremony_id).await?;
    Ok(CeremonyLifecycle {
        state: CeremonyLifecycleState::TimedOut,
        status,
        attempts: policy.max_attempts,
    })
}

/// Set recovery state in ViewState
///
/// Signal forwarding automatically propagates to RECOVERY_SIGNAL.
async fn set_recovery_state(
    app_core: &Arc<RwLock<AppCore>>,
    state: RecoveryState,
) -> Result<(), AuraError> {
    let mut core = app_core.write().await;
    core.views_mut().set_recovery(state);
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
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let status = get_recovery_status(&app_core).await.unwrap();
        assert!(status.active_recovery.is_none());
        assert!(status.guardians.is_empty());
    }

    #[tokio::test]
    async fn test_set_recovery_state() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        // Set recovery state with active recovery process
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

        // Update ViewState directly
        set_recovery_state(&app_core, state.clone()).await.unwrap();

        // Verify state was set
        let retrieved = get_recovery_status(&app_core).await.unwrap();
        assert!(retrieved.active_recovery.is_some());
        assert_eq!(
            retrieved.active_recovery.as_ref().unwrap().id,
            "ceremony-123"
        );
    }
}
