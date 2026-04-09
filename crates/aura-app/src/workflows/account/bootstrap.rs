use super::validation::{prepare_pending_account_bootstrap, validate_nickname_suggestion};
use crate::ui_contract::{
    OperationId, OperationInstanceId, SemanticFailureCode, SemanticFailureDomain,
    SemanticOperationError, SemanticOperationKind, SemanticOperationPhase,
};
use crate::views::PendingAccountBootstrap;
#[cfg(not(target_arch = "wasm32"))]
use crate::workflows::runtime::{execute_with_runtime_retry_budget, workflow_retry_policy};
use crate::workflows::{
    runtime::{
        execute_with_runtime_timeout_budget, require_runtime, timeout_runtime_call,
        warn_workflow_timeout, workflow_timeout_budget,
    },
    semantic_facts::SemanticWorkflowOwner,
    settings, system,
};
use crate::AppCore;
use async_lock::RwLock;
#[cfg(not(target_arch = "wasm32"))]
use aura_core::RetryRunError;
use aura_core::{AuraError, OperationContext, TimeoutBudgetError, TimeoutRunError, TraceContext};
use std::sync::Arc;
use std::time::Duration;

const ACCOUNT_RUNTIME_QUERY_TIMEOUT: Duration = Duration::from_millis(5_000);
const ACCOUNT_RUNTIME_OPERATION_TIMEOUT: Duration = Duration::from_millis(30_000);

async fn run_account_bootstrap_stage<T, F, Fut>(
    app_core: &Arc<RwLock<AppCore>>,
    stage: &'static str,
    duration: Duration,
    operation: F,
) -> Result<T, AuraError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<T, AuraError>>,
{
    let runtime = require_runtime(app_core).await?;
    let budget = workflow_timeout_budget(&runtime, duration)
        .await
        .map_err(AuraError::from)?;
    match execute_with_runtime_timeout_budget(&runtime, &budget, operation).await {
        Ok(value) => Ok(value),
        Err(TimeoutRunError::Timeout(TimeoutBudgetError::DeadlineExceeded { .. })) => {
            warn_workflow_timeout(
                "finalize_runtime_account_bootstrap",
                stage,
                budget.timeout_ms(),
            );
            Err(AuraError::from(
                crate::workflows::error::WorkflowError::TimedOut {
                    operation: "finalize_runtime_account_bootstrap",
                    stage,
                    timeout_ms: budget.timeout_ms(),
                },
            ))
        }
        Err(TimeoutRunError::Timeout(error)) => Err(AuraError::from(error)),
        Err(TimeoutRunError::Operation(error)) => Err(error),
    }
}

/// Shared outcome of reconciling pending first-run runtime bootstrap state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingRuntimeBootstrapResolution {
    /// Whether the runtime has a persisted account configuration after reconciliation.
    pub account_ready: bool,
    /// What the reconciliation step had to do.
    pub action: PendingRuntimeBootstrapAction,
}

/// Action taken while reconciling pending first-run runtime bootstrap state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingRuntimeBootstrapAction {
    /// Nothing was pending.
    None,
    /// Pending bootstrap metadata was consumed to initialize the runtime account.
    InitializedFromPending,
    /// Pending bootstrap metadata was stale because the runtime account already existed.
    ClearedStalePending,
}

/// Returns true when a runtime-backed account configuration exists.
pub async fn has_runtime_account_config(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<bool, AuraError> {
    let runtime = require_runtime(app_core).await?;
    timeout_runtime_call(
        &runtime,
        "has_runtime_account_config",
        "has_account_config",
        ACCOUNT_RUNTIME_QUERY_TIMEOUT,
        || runtime.has_account_config(),
    )
    .await
    .map_err(|e| AuraError::from(super::super::error::runtime_call("check account config", e)))?
    .map_err(|e| AuraError::from(super::super::error::runtime_call("check account config", e)))
}

/// Returns true when runtime bootstrap has completed (account config exists).
pub async fn has_runtime_bootstrapped_account(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<bool, AuraError> {
    has_runtime_account_config(app_core).await
}

/// Persist first-run account configuration for the current runtime authority.
pub async fn initialize_runtime_account(
    app_core: &Arc<RwLock<AppCore>>,
    nickname_suggestion: String,
) -> Result<(), AuraError> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::account_create(),
        None,
        SemanticOperationKind::CreateAccount,
    );
    initialize_runtime_account_owned(app_core, nickname_suggestion, &owner, None).await
}

async fn fail_initialize_runtime_account<T>(
    owner: &SemanticWorkflowOwner,
    detail: impl Into<String>,
) -> Result<T, AuraError> {
    let error = SemanticOperationError::new(
        SemanticFailureDomain::Internal,
        SemanticFailureCode::InternalError,
    )
    .with_detail(detail.into());
    owner.publish_failure(error.clone()).await?;
    Err(AuraError::agent(error.detail.unwrap_or_else(|| {
        "initialize runtime account failed".to_string()
    })))
}

#[aura_macros::semantic_owner(
    owner = "initialize_runtime_account_owned",
    wrapper = "initialize_runtime_account",
    terminal = "publish_success_with",
    postcondition = "account_created",
    proof = crate::workflows::semantic_facts::AccountCreatedProof,
    authoritative_inputs = "runtime",
    child_ops = "",
    depends_on = "settings_refreshed,account_refreshed",
    category = "move_owned"
)]
async fn initialize_runtime_account_owned(
    app_core: &Arc<RwLock<AppCore>>,
    nickname_suggestion: String,
    owner: &SemanticWorkflowOwner,
    _operation_context: Option<
        &mut OperationContext<OperationId, OperationInstanceId, TraceContext>,
    >,
) -> Result<(), AuraError> {
    owner
        .publish_phase(SemanticOperationPhase::WorkflowDispatched)
        .await?;

    let pending_bootstrap = prepare_pending_account_bootstrap(&nickname_suggestion)?;
    let runtime = require_runtime(app_core).await?;
    let init_result = timeout_runtime_call(
        &runtime,
        "initialize_runtime_account",
        "initialize_account",
        ACCOUNT_RUNTIME_OPERATION_TIMEOUT,
        || runtime.initialize_account(&pending_bootstrap.nickname_suggestion),
    )
    .await
    .map_err(|e| AuraError::from(super::super::error::runtime_call("initialize account", e)))
    .and_then(|result| {
        result.map_err(|e| {
            AuraError::from(super::super::error::runtime_call("initialize account", e))
        })
    })
    .map_err(|error| AuraError::agent(error.to_string()));
    if let Err(error) = init_result {
        return fail_initialize_runtime_account(owner, error.to_string()).await;
    }

    if let Err(error) =
        finalize_runtime_account_bootstrap_inner(app_core, pending_bootstrap.nickname_suggestion)
            .await
    {
        return fail_initialize_runtime_account(owner, error.to_string()).await;
    }

    owner
        .publish_success_with(crate::workflows::semantic_facts::issue_account_created_proof())
        .await?;
    Ok(())
}

/// Reconcile pending first-run runtime bootstrap metadata against the current runtime state.
pub async fn reconcile_pending_runtime_account_bootstrap(
    app_core: &Arc<RwLock<AppCore>>,
    pending_bootstrap: Option<PendingAccountBootstrap>,
) -> Result<PendingRuntimeBootstrapResolution, AuraError> {
    let account_ready = has_runtime_bootstrapped_account(app_core).await?;
    match (account_ready, pending_bootstrap) {
        (false, Some(pending_bootstrap)) => {
            initialize_runtime_account(app_core, pending_bootstrap.nickname_suggestion).await?;
            Ok(PendingRuntimeBootstrapResolution {
                account_ready: true,
                action: PendingRuntimeBootstrapAction::InitializedFromPending,
            })
        }
        (true, Some(_)) => {
            #[cfg(feature = "signals")]
            ensure_note_to_self_on_login(app_core).await;
            Ok(PendingRuntimeBootstrapResolution {
                account_ready: true,
                action: PendingRuntimeBootstrapAction::ClearedStalePending,
            })
        }
        (ready, None) => {
            #[cfg(feature = "signals")]
            if ready {
                ensure_note_to_self_on_login(app_core).await;
            }
            Ok(PendingRuntimeBootstrapResolution {
                account_ready: ready,
                action: PendingRuntimeBootstrapAction::None,
            })
        }
    }
}

/// Best-effort Note-to-self channel provisioning for existing accounts.
#[cfg(feature = "signals")]
async fn ensure_note_to_self_on_login(app_core: &Arc<RwLock<AppCore>>) {
    let result: Result<(), AuraError> = async {
        let runtime = require_runtime(app_core).await?;
        let authority_id = runtime.authority_id();
        let timestamp_ms = crate::workflows::time::current_time_ms(app_core)
            .await
            .map_err(|e| AuraError::agent(e.to_string()))?;
        super::super::messaging::ensure_runtime_note_to_self_channel(
            app_core,
            &runtime,
            authority_id,
            timestamp_ms,
        )
        .await?;
        Ok(())
    }
    .await;
    let _ = result;
}

/// Complete first-run runtime bootstrap after account metadata already exists.
pub async fn finalize_runtime_account_bootstrap(
    app_core: &Arc<RwLock<AppCore>>,
    nickname_suggestion: String,
) -> Result<(), AuraError> {
    finalize_runtime_account_bootstrap_inner(app_core, nickname_suggestion).await
}

async fn finalize_runtime_account_bootstrap_inner(
    app_core: &Arc<RwLock<AppCore>>,
    nickname_suggestion: String,
) -> Result<(), AuraError> {
    let _nickname_suggestion = validate_nickname_suggestion(&nickname_suggestion)
        .map_err(|error| AuraError::invalid(error.to_string()))?;
    let _authority_id = {
        let core = app_core.read().await;
        core.runtime()
            .map(|runtime| runtime.authority_id())
            .or_else(|| core.authority().copied())
    }
    .ok_or_else(|| AuraError::permission_denied("Authority not set"))?;

    #[cfg(not(target_arch = "wasm32"))]
    {
        const SIGNING_KEY_ATTEMPTS: usize = 40;
        const BOOTSTRAP_RETRY_MS: u64 = 250;
        let retry_policy = workflow_retry_policy(
            SIGNING_KEY_ATTEMPTS as u32,
            Duration::from_millis(BOOTSTRAP_RETRY_MS),
            Duration::from_millis(BOOTSTRAP_RETRY_MS),
        )?;
        execute_with_runtime_retry_budget(
            &require_runtime(app_core).await?,
            &retry_policy,
            |_attempt| async {
                let runtime = {
                    let core = app_core.read().await;
                    core.runtime().cloned()
                };
                if let Some(runtime) = runtime {
                    if timeout_runtime_call(
                        &runtime,
                        "finalize_runtime_account_bootstrap",
                        "bootstrap_signing_keys",
                        ACCOUNT_RUNTIME_OPERATION_TIMEOUT,
                        || runtime.bootstrap_signing_keys(),
                    )
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .is_some()
                    {
                        return Ok(());
                    }
                }
                Err(AuraError::from(
                    crate::workflows::error::WorkflowError::Precondition(
                        "runtime signing keys not yet bootstrapped",
                    ),
                ))
            },
        )
        .await
        .map_err(|error| match error {
            RetryRunError::Timeout(timeout_error) => AuraError::from(timeout_error),
            RetryRunError::AttemptsExhausted { last_error, .. } => last_error,
        })?;
    }

    #[cfg(target_arch = "wasm32")]
    {
        if let Some(runtime) = {
            let core = app_core.read().await;
            core.runtime().cloned()
        } {
            timeout_runtime_call(
                &runtime,
                "finalize_runtime_account_bootstrap",
                "bootstrap_signing_keys",
                ACCOUNT_RUNTIME_OPERATION_TIMEOUT,
                || runtime.bootstrap_signing_keys(),
            )
            .await
            .map_err(|e| AuraError::agent(e.to_string()))?
            .map_err(|e| AuraError::agent(e.to_string()))?;
        }
    }

    #[cfg(feature = "signals")]
    run_account_bootstrap_stage(
        app_core,
        "ensure_note_to_self_channel",
        ACCOUNT_RUNTIME_OPERATION_TIMEOUT,
        || async {
            let runtime = require_runtime(app_core).await?;
            let timestamp_ms = crate::workflows::time::current_time_ms(app_core)
                .await
                .map_err(|e| AuraError::agent(e.to_string()))?;
            super::super::messaging::ensure_runtime_note_to_self_channel(
                app_core,
                &runtime,
                _authority_id,
                timestamp_ms,
            )
            .await?;
            Ok(())
        },
    )
    .await?;

    run_account_bootstrap_stage(
        app_core,
        "refresh_settings_from_runtime",
        ACCOUNT_RUNTIME_QUERY_TIMEOUT,
        || async { settings::refresh_settings_from_runtime(app_core).await },
    )
    .await?;
    run_account_bootstrap_stage(
        app_core,
        "refresh_account",
        ACCOUNT_RUNTIME_OPERATION_TIMEOUT,
        || async { system::refresh_account(app_core).await },
    )
    .await?;
    Ok(())
}
