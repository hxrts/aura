//! Runtime-free account staging has the same app semantic owner as runtime creation.
use super::bootstrap::fail_initialize_runtime_account;
use super::prepare_pending_account_bootstrap;
use crate::ui_contract::{
    OperationId, OperationInstanceId, SemanticOperationKind, SemanticOperationPhase,
    WorkflowTerminalOutcome,
};
use crate::workflows::config::PENDING_ACCOUNT_BOOTSTRAP_FILENAME;
use crate::workflows::semantic_facts::SemanticWorkflowOwner;
use crate::{views::AccountConfig, AppCore};
use async_lock::RwLock;
use aura_core::effects::{PhysicalTimeEffects, RandomEffects, StorageCoreEffects};
use aura_core::{
    execute_with_timeout_budget, AuraError, AuthorityId, ContextId, TimeoutBudget, TimeoutRunError,
};
use std::{sync::Arc, time::Duration};

/// Execute the actual configured storage producer; callers cannot supply a success witness.
/// The pending/config writes must both acknowledge before this owner publishes success.
pub async fn stage_runtime_free_account_with_terminal_status(
    app: &Arc<RwLock<AppCore>>,
    storage: &impl StorageCoreEffects,
    time: &impl PhysicalTimeEffects,
    random: &impl RandomEffects,
    nickname: String,
    instance: Option<OperationInstanceId>,
) -> WorkflowTerminalOutcome<(AuthorityId, ContextId)> {
    let owner = SemanticWorkflowOwner::new(
        app,
        OperationId::account_create(),
        instance,
        SemanticOperationKind::CreateAccount,
    );
    let result =
        stage_runtime_free_account_owned(app, storage, time, random, nickname, &owner, None).await;
    let result = match result {
        Ok(value) => Ok(value),
        Err(cause) if owner.terminal_status().await.is_none() => {
            fail_initialize_runtime_account(&owner, cause).await
        }
        Err(cause) => Err(cause),
    };
    WorkflowTerminalOutcome {
        result,
        terminal: owner.terminal_status().await,
    }
}

#[aura_macros::semantic_owner(
    owner = "stage_runtime_free_account_owned",
    wrapper = "stage_runtime_free_account_with_terminal_status",
    terminal = "publish_success_with",
    postcondition = "account_created",
    proof = crate::workflows::semantic_facts::AccountCreatedProof,
    authoritative_inputs = "app,storage,time,random",
    child_ops = "",
    depends_on = "",
    category = "move_owned"
)]
async fn stage_runtime_free_account_owned(
    app: &Arc<RwLock<AppCore>>,
    storage: &impl StorageCoreEffects,
    time: &impl PhysicalTimeEffects,
    random: &impl RandomEffects,
    nickname: String,
    owner: &SemanticWorkflowOwner,
    _operation_context: Option<
        &mut aura_core::OperationContext<OperationId, OperationInstanceId, aura_core::TraceContext>,
    >,
) -> Result<(AuthorityId, ContextId), AuraError> {
    owner
        .publish_phase(SemanticOperationPhase::WorkflowDispatched)
        .await?;
    if app.read().await.runtime().is_some() {
        return Err(AuraError::invalid(
            "Runtime-free staging requires the original unattached application",
        ));
    }
    let pending = prepare_pending_account_bootstrap(&nickname)?;
    let start = time
        .physical_time()
        .await
        .map_err(|cause| AuraError::Internal {
            message: "Begin account staging window".into(),
            source: Some(Arc::new(cause)),
        })?;
    let budget = TimeoutBudget::from_start_and_timeout(&start, Duration::from_secs(30))
        .map_err(AuraError::from)?;
    let outcome = execute_with_timeout_budget(time, &budget, || async {
        let authority = AuthorityId::new_from_entropy(random.random_bytes_32().await);
        let context = ContextId::new_from_entropy(random.random_bytes_32().await);
        let config = AccountConfig {
            authority_id: authority,
            context_id: context,
            nickname_suggestion: Some(pending.nickname_suggestion.clone()),
            created_at: start.ts_ms,
        };
        let pending_bytes = serde_json::to_vec(&pending).map_err(|cause| AuraError::Internal {
            message: "Encode pending bootstrap".into(),
            source: Some(Arc::new(cause)),
        })?;
        let config_bytes =
            serde_json::to_vec_pretty(&config).map_err(|cause| AuraError::Internal {
                message: "Encode staged account".into(),
                source: Some(Arc::new(cause)),
            })?;
        storage
            .store(PENDING_ACCOUNT_BOOTSTRAP_FILENAME, pending_bytes)
            .await
            .map_err(|cause| AuraError::Storage {
                message: "Persist pending bootstrap".into(),
                source: Some(Arc::new(cause)),
            })?;
        storage
            .store(crate::workflows::config::ACCOUNT_FILENAME, config_bytes)
            .await
            .map_err(|cause| AuraError::Storage {
                message: "Persist staged account".into(),
                source: Some(Arc::new(cause)),
            })?;
        Ok::<_, AuraError>((authority, context))
    })
    .await
    .map_err(|cause| match cause {
        TimeoutRunError::Operation(cause) => cause,
        TimeoutRunError::Timeout(cause) => AuraError::from(cause),
    })?;
    owner
        .publish_success_with(crate::workflows::semantic_facts::issue_account_created_proof())
        .await?;
    Ok(outcome)
}
