//! Recovery Workflow - Portable Business Logic
//!
//! This module contains guardian recovery operations that are portable
//! across all frontends. Uses typed reactive signals for state reads/writes.

use crate::views::contacts::{Contact, ReadReceiptPolicy};
use crate::workflows::ceremonies::{
    CeremonyLifecycle, CeremonyLifecycleState, CeremonyPollPolicy, CeremonyStatusLike,
};
use crate::workflows::parse::parse_authority_id;
use crate::workflows::runtime::require_runtime;
use crate::workflows::snapshot_policy::recovery_snapshot;
use crate::workflows::state_helpers::{
    update_contacts_projection_observed, update_recovery_projection_observed,
};
use crate::workflows::time::current_time_ms;
use crate::{
    runtime_bridge::CeremonyStatus,
    views::recovery::{
        Guardian, GuardianStatus, RecoveryProcess, RecoveryProcessStatus, RecoveryState,
    },
    AppCore,
};
use async_lock::RwLock;
use aura_core::{
    types::{AuthorityId, FrostThreshold},
    AttemptBudget, AuraError, CeremonyId, Hash32,
};
use aura_journal::fact::RelationalFact;
use aura_journal::ProtocolRelationalFact;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

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
    guardian_ids: Vec<AuthorityId>,
    threshold_k: FrostThreshold,
) -> Result<CeremonyId, AuraError> {
    let runtime = { require_runtime(app_core).await? };

    if guardian_ids.is_empty() {
        return Err(AuraError::invalid("Guardian list cannot be empty"));
    }

    let threshold_required = usize::from(threshold_k.value());
    if guardian_ids.len() < threshold_required {
        return Err(AuraError::invalid(format!(
            "Threshold {} exceeds guardian count {}",
            threshold_k.value(),
            guardian_ids.len()
        )));
    }

    let initiated_at = current_time_ms(app_core).await?;

    let guardians = guardian_ids
        .iter()
        .map(|authority| crate::views::recovery::Guardian {
            id: *authority,
            name: String::new(),
            status: crate::views::recovery::GuardianStatus::Pending,
            added_at: initiated_at,
            last_seen: None,
        })
        .collect::<Vec<_>>();

    // Initiate guardian ceremony via runtime bridge
    let total_n = u16::try_from(guardian_ids.len()).map_err(|_| {
        AuraError::invalid(format!(
            "Guardian count {} exceeds supported maximum {}",
            guardian_ids.len(),
            u16::MAX
        ))
    })?;
    let ceremony_id = runtime
        .initiate_guardian_ceremony(threshold_k, total_n, &guardian_ids)
        .await
        .map_err(|e| super::error::runtime_call("start recovery", e))?;

    // Create recovery state with initial process
    let recovery_process = RecoveryProcess {
        id: ceremony_id.clone(),
        account_id: runtime.authority_id(),
        status: RecoveryProcessStatus::WaitingForApprovals,
        approvals_received: 0,
        approvals_required: u32::from(threshold_k.value()),
        approved_by: Vec::new(),
        approvals: Vec::new(),
        initiated_at,
        expires_at: None,
        progress: 0,
    };

    let state = RecoveryState::from_parts(
        guardians,
        u32::from(threshold_k.value()),
        Some(recovery_process),
        Vec::new(), // pending_requests
        Vec::new(), // guardian_bindings
    );

    // Update ViewState - signal forwarding auto-propagates to RECOVERY_SIGNAL.
    //
    // If this fails the ceremony is already initiated at the runtime but local
    // state never reflects it.  Attempt best-effort cancellation so the runtime
    // doesn't have a dangling ceremony.
    if let Err(state_error) = set_recovery_state(app_core, state).await {
        #[cfg(feature = "instrumented")]
        tracing::error!(
            error = %state_error,
            ceremony_id = %ceremony_id,
            "recovery state write failed after ceremony init — attempting cancellation"
        );
        let core = app_core.read().await;
        let _ = core.cancel_key_rotation_ceremony(&ceremony_id).await;
        return Err(state_error);
    }

    Ok(ceremony_id)
}

/// Start recovery using the current recovery state in RECOVERY_SIGNAL.
///
/// **What it does**: Validates guardian set + threshold from state and starts ceremony.
/// **Signal pattern**: Updates RECOVERY_SIGNAL directly
pub async fn start_recovery_from_state(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<CeremonyId, AuraError> {
    let state = get_recovery_status(app_core).await?;

    if state.guardian_count() == 0 {
        return Err(super::error::WorkflowError::Precondition(
            "No guardians configured. Add guardians in Threshold settings first.",
        )
        .into());
    }

    if state.active_recovery().is_some() {
        return Err(
            super::error::WorkflowError::Precondition("Recovery already in progress").into(),
        );
    }

    let guardian_ids: Vec<AuthorityId> = state.all_guardians().map(|g| g.id).collect();

    let threshold_value = u16::try_from(state.threshold()).map_err(|_| {
        AuraError::invalid(format!(
            "Invalid recovery threshold {}: exceeds u16 range",
            state.threshold()
        ))
    })?;
    let threshold = FrostThreshold::new(threshold_value).map_err(|e| {
        AuraError::invalid(format!(
            "Invalid recovery threshold {}: {e}",
            state.threshold()
        ))
    })?;

    start_recovery(app_core, guardian_ids, threshold).await
}

/// Toggle guardian membership for a contact.
///
/// Returns `true` when the contact became a guardian, `false` when guardian
/// status was revoked.
///
/// # Multi-step atomicity
///
/// This function performs up to three sequential mutations:
/// 1. Create guardian invitation on the runtime (when adding)
/// 2. Update recovery state
/// 3. Update contacts state
///
/// If step 2 fails after step 1, the runtime invitation is leaked.  If
/// step 3 fails after steps 1+2, contacts state is inconsistent with
/// recovery state.  Full transactional rollback is not yet implemented;
/// callers should treat errors from this function as requiring a manual
/// consistency check (e.g. `refresh_account`).
pub async fn toggle_guardian_contact(
    app_core: &Arc<RwLock<AppCore>>,
    contact_id: &str,
    timestamp_ms: u64,
) -> Result<bool, AuraError> {
    let contact = parse_authority_id(contact_id)?;
    let was_guardian = get_recovery_status(app_core).await?.has_guardian(&contact);

    if !was_guardian {
        let runtime = require_runtime(app_core).await?;
        let subject = runtime.authority_id();
        runtime
            .create_guardian_invitation(contact, subject, None, None)
            .await
            .map_err(|e| super::error::runtime_call("create guardian invitation", e))?;
    }

    // OWNERSHIP: observed-display-update
    update_recovery_projection_observed(app_core, |state| -> Result<(), AuraError> {
        if was_guardian {
            let _ = state.revoke_guardian(&contact);
        } else if let Some(existing) = state.guardian_mut(&contact) {
            existing.status = GuardianStatus::Pending;
            if existing.added_at == 0 {
                existing.added_at = timestamp_ms;
            }
        } else {
            state.upsert_guardian(Guardian {
                id: contact,
                name: String::new(),
                status: GuardianStatus::Pending,
                added_at: timestamp_ms,
                last_seen: None,
            });
        }

        let guardian_count = u32::try_from(state.guardian_count()).map_err(|_| {
            AuraError::invalid(format!(
                "Guardian count {} exceeds supported maximum {}",
                state.guardian_count(),
                u32::MAX
            ))
        })?;
        if guardian_count == 0 {
            state.set_threshold(0);
        } else if state.threshold() == 0 {
            state.set_threshold(1);
        } else if state.threshold() > guardian_count {
            state.set_threshold(guardian_count);
        }

        Ok(())
    })
    .await??;

    // OWNERSHIP: observed-display-update
    update_contacts_projection_observed(app_core, |state| {
        if let Some(existing) = state.contact_mut(&contact) {
            existing.is_guardian = !was_guardian;
        } else {
            state.apply_contact(Contact {
                id: contact,
                nickname: String::new(),
                nickname_suggestion: None,
                is_guardian: !was_guardian,
                is_member: false,
                last_interaction: Some(timestamp_ms),
                is_online: false,
                read_receipt_policy: ReadReceiptPolicy::default(),
            });
        }
    })
    .await?;

    Ok(!was_guardian)
}

/// Approve a recovery request as a guardian
///
/// **What it does**: Records guardian approval for recovery ceremony
/// **Returns**: Unit result
/// **Signal pattern**: RuntimeBridge handles signal emission
pub async fn approve_recovery(
    app_core: &Arc<RwLock<AppCore>>,
    ceremony_id: &CeremonyId,
) -> Result<(), AuraError> {
    let runtime = { require_runtime(app_core).await? };

    runtime
        .respond_to_guardian_ceremony(ceremony_id, true, None)
        .await
        .map_err(|e| super::error::runtime_call("approve recovery", e).into())
}

/// Commit a GuardianBinding fact via the runtime bridge (demo/testing helper).
pub async fn commit_guardian_binding(
    app_core: &Arc<RwLock<AppCore>>,
    account_id: AuthorityId,
    guardian_id: AuthorityId,
    binding_hash: Hash32,
) -> Result<(), AuraError> {
    let runtime = { require_runtime(app_core).await? };

    let fact = RelationalFact::Protocol(ProtocolRelationalFact::GuardianBinding {
        account_id,
        guardian_id,
        binding_hash,
    });

    runtime
        .commit_relational_facts(&[fact])
        .await
        .map_err(|e| super::error::runtime_call("commit guardian binding", e).into())
}

/// Dispute a recovery request
///
/// **What it does**: Files a dispute against a recovery ceremony
/// **Returns**: Unit result
/// **Signal pattern**: RuntimeBridge handles signal emission
pub async fn dispute_recovery(
    app_core: &Arc<RwLock<AppCore>>,
    ceremony_id: &CeremonyId,
    reason: String,
) -> Result<(), AuraError> {
    let runtime = { require_runtime(app_core).await? };

    runtime
        .respond_to_guardian_ceremony(ceremony_id, false, Some(reason))
        .await
        .map_err(|e| super::error::runtime_call("dispute recovery", e).into())
}

/// Get current recovery status
///
/// **What it does**: Reads recovery state from ViewState
/// **Returns**: Current recovery state
/// **Signal pattern**: Read-only operation (no emission)
// OWNERSHIP: observed
pub async fn get_recovery_status(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<RecoveryState, AuraError> {
    Ok(recovery_snapshot(app_core).await)
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
    ceremony_id: &CeremonyId,
) -> Result<CeremonyStatus, AuraError> {
    let runtime = { require_runtime(app_core).await? };

    runtime
        .get_ceremony_status(ceremony_id)
        .await
        .map_err(|e| super::error::runtime_call("get ceremony status", e).into())
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
    ceremony_id: CeremonyId,
    policy: CeremonyPollPolicy,
    mut on_update: impl FnMut(&CeremonyStatus) + Send,
    mut sleep_fn: SleepFn,
) -> Result<CeremonyLifecycle<CeremonyStatus>, AuraError>
where
    SleepFn: FnMut(Duration) -> SleepFut + Send,
    SleepFut: Future<Output = ()> + Send,
{
    let mut attempts = AttemptBudget::new(policy.max_attempts);
    while attempts.can_attempt() {
        let attempt = attempts
            .record_attempt()
            .map_err(AuraError::from)?
            .saturating_add(1);
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
    // OWNERSHIP: observed-display-update
    update_recovery_projection_observed(app_core, |recovery_state| {
        *recovery_state = state;
    })
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::recovery::{
        Guardian, GuardianStatus, RecoveryProcess, RecoveryProcessStatus,
    };
    use crate::AppConfig;
    use aura_core::types::identifiers::AuthorityId;

    #[tokio::test]
    async fn test_get_recovery_status_default() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let status = get_recovery_status(&app_core).await.unwrap();
        assert!(status.active_recovery().is_none());
        assert_eq!(status.guardian_count(), 0);
    }

    #[tokio::test]
    async fn test_set_recovery_state() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        // Set recovery state with active recovery process
        let guardians = vec![Guardian {
            id: AuthorityId::new_from_entropy([1u8; 32]),
            name: "Alice".to_string(),
            status: GuardianStatus::Active,
            added_at: 1000,
            last_seen: Some(2000),
        }];
        let active_recovery = Some(RecoveryProcess {
            id: CeremonyId::new("ceremony-123"),
            account_id: AuthorityId::new_from_entropy([1u8; 32]),
            status: RecoveryProcessStatus::WaitingForApprovals,
            approvals_received: 0,
            approvals_required: 2,
            approved_by: vec![],
            approvals: vec![],
            initiated_at: 1000,
            expires_at: Some(2000),
            progress: 0,
        });
        let state = RecoveryState::from_parts(guardians, 2, active_recovery, vec![], vec![]);

        // Update ViewState directly
        set_recovery_state(&app_core, state.clone()).await.unwrap();

        // Verify state was set
        let retrieved = get_recovery_status(&app_core).await.unwrap();
        assert!(retrieved.active_recovery().is_some());
        assert_eq!(
            retrieved.active_recovery().unwrap().id,
            CeremonyId::new("ceremony-123")
        );
    }
}
