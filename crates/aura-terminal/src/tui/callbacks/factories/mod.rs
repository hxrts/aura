//! # Callback Factories
//!
//! Factory functions that create domain-specific callbacks.
//! Each factory takes an `IoContext` and `UiUpdateSender` and returns
//! a struct containing all callbacks for that domain.

mod chat;
mod contacts;
mod invitation;
mod recovery;
mod settings;

pub use chat::ChatCallbacks;
pub use contacts::ContactsCallbacks;
pub use invitation::InvitationsCallbacks;
pub use recovery::RecoveryCallbacks;
pub use settings::{NeighborhoodCallbacks, SettingsCallbacks};

use std::future::Future;
use std::sync::Arc;

use crate::tui::components::copy_to_clipboard;
use crate::tui::components::ToastMessage;
use crate::tui::context::IoContext;
use crate::tui::effects::EffectCommand;
use crate::tui::semantic_lifecycle::{
    apply_handed_off_terminal_status, LocalTerminalOperationOwner, SemanticOperationTransferScope,
    WorkflowHandoffOperationOwner,
};
use crate::tui::types::{AccessLevel, MfaPolicy};
use crate::tui::updates::{UiOperation, UiUpdate, UiUpdateSender};
use async_lock::RwLock;
use aura_app::ui::workflows::invitation::import_invitation_details;
use aura_app::ui::workflows::strong_command::{
    classify_terminal_execution_error, CommandTerminalOutcomeStatus, CommandTerminalReasonCode,
};
use aura_app::ui_contract::{
    OperationId, SemanticFailureCode, SemanticFailureDomain, SemanticOperationError,
    SemanticOperationKind, SemanticOperationPhase, SemanticOperationStatus,
    WorkflowTerminalOutcome,
};
use aura_core::AuthorityId;
use futures::FutureExt;

use super::types::*;

use crate::tui::updates::send_ui_update_required;

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct CallbackFactoryRuntime {
    ctx: Arc<IoContext>,
    tx: UiUpdateSender,
}

impl CallbackFactoryRuntime {
    #[must_use]
    pub fn new(ctx: Arc<IoContext>, tx: UiUpdateSender) -> Self {
        Self { ctx, tx }
    }

    fn ctx(&self) -> Arc<IoContext> {
        self.ctx.clone()
    }

    fn tx(&self) -> UiUpdateSender {
        self.tx.clone()
    }
}

#[derive(Clone)]
struct WorkflowHandoffSpec {
    operation_id: OperationId,
    kind: SemanticOperationKind,
    scope: SemanticOperationTransferScope,
    toast_scope: &'static str,
    failure_prefix: &'static str,
    panic_context: &'static str,
}

impl WorkflowHandoffSpec {
    fn new(
        operation_id: OperationId,
        kind: SemanticOperationKind,
        scope: SemanticOperationTransferScope,
        toast_scope: &'static str,
        failure_prefix: &'static str,
        panic_context: &'static str,
    ) -> Self {
        Self {
            operation_id,
            kind,
            scope,
            toast_scope,
            failure_prefix,
            panic_context,
        }
    }
}

#[allow(clippy::needless_pass_by_value)] // Arc clone pattern for task spawning
fn spawn_ctx<F>(ctx: Arc<IoContext>, fut: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    ctx.tasks().spawn(fut);
}

async fn send_ui_update_reliable(tx: &UiUpdateSender, update: UiUpdate) {
    send_ui_update_required(tx, update).await;
}

fn enqueue_ui_update_required(ctx: Arc<IoContext>, tx: UiUpdateSender, update: UiUpdate) {
    spawn_ctx(ctx, async move {
        send_ui_update_required(&tx, update).await;
    });
}

fn panic_detail(panic_context: &'static str, panic: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = panic.downcast_ref::<&str>() {
        format!("{panic_context} panicked: {message}")
    } else if let Some(message) = panic.downcast_ref::<String>() {
        format!("{panic_context} panicked: {message}")
    } else {
        format!("{panic_context} panicked")
    }
}

async fn emit_error_toast(tx: &UiUpdateSender, scope: &'static str, message: impl Into<String>) {
    send_ui_update_reliable(
        tx,
        UiUpdate::ToastAdded(ToastMessage::error(scope, message)),
    )
    .await;
}

fn spawn_local_terminal_result_callback<
    T,
    Action,
    ActionFut,
    Success,
    SuccessFut,
    Failure,
    FailureFut,
>(
    ctx: Arc<IoContext>,
    tx: UiUpdateSender,
    operation: LocalTerminalOperationOwner,
    panic_context: &'static str,
    action: Action,
    on_success: Success,
    on_failure: Failure,
) where
    T: Clone + Send + 'static,
    Action: FnOnce(Arc<IoContext>) -> ActionFut + Send + 'static,
    ActionFut: Future<Output = crate::error::TerminalResult<T>> + Send + 'static,
    Success: FnOnce(UiUpdateSender, T) -> SuccessFut + Send + 'static,
    SuccessFut: Future<Output = ()> + Send + 'static,
    Failure: FnOnce(UiUpdateSender, crate::error::TerminalError) -> FailureFut + Send + 'static,
    FailureFut: Future<Output = ()> + Send + 'static,
{
    spawn_ctx(ctx.clone(), async move {
        match std::panic::AssertUnwindSafe(action(ctx))
            .catch_unwind()
            .await
        {
            Ok(Ok(value)) => {
                operation.succeed().await;
                if let Err(panic) = std::panic::AssertUnwindSafe(on_success(tx.clone(), value))
                    .catch_unwind()
                    .await
                {
                    tracing::error!("{}", panic_detail(panic_context, panic.as_ref()));
                }
            }
            Ok(Err(error)) => {
                operation.fail(error.to_string()).await;
                on_failure(tx, error).await;
            }
            Err(_) => {
                let error =
                    crate::error::TerminalError::Operation(format!("panic in {panic_context}"));
                operation.fail(error.to_string()).await;
                on_failure(tx, error).await;
            }
        }
    });
}

// Observed callbacks may only use this helper for terminal-local adaptation.
// Parity-critical ownership handoff belongs in shell dispatch or owner-typed callbacks.
fn spawn_observed_adaptation_dispatch_callback<Success, SuccessFut, Failure, FailureFut>(
    ctx: Arc<IoContext>,
    tx: UiUpdateSender,
    command: EffectCommand,
    on_success: Success,
    on_failure: Failure,
) where
    Success: FnOnce(UiUpdateSender) -> SuccessFut + Send + 'static,
    SuccessFut: Future<Output = ()> + Send + 'static,
    Failure: FnOnce(crate::error::TerminalError) -> FailureFut + Send + 'static,
    FailureFut: Future<Output = ()> + Send + 'static,
{
    let dispatch_ctx = ctx.clone();
    spawn_ctx(ctx, async move {
        match dispatch_ctx.dispatch(command).await {
            Ok(()) => on_success(tx).await,
            Err(error) => on_failure(error).await,
        }
    });
}

fn spawn_observed_result_callback<T, Action, ActionFut, Success, SuccessFut, Failure, FailureFut>(
    ctx: Arc<IoContext>,
    tx: UiUpdateSender,
    panic_context: &'static str,
    action: Action,
    on_success: Success,
    on_failure: Failure,
) where
    T: Clone + Send + 'static,
    Action: FnOnce(Arc<IoContext>) -> ActionFut + Send + 'static,
    ActionFut: Future<Output = crate::error::TerminalResult<T>> + Send + 'static,
    Success: FnOnce(UiUpdateSender, T) -> SuccessFut + Send + 'static,
    SuccessFut: Future<Output = ()> + Send + 'static,
    Failure: FnOnce(UiUpdateSender, crate::error::TerminalError) -> FailureFut + Send + 'static,
    FailureFut: Future<Output = ()> + Send + 'static,
{
    spawn_ctx(ctx.clone(), async move {
        match std::panic::AssertUnwindSafe(action(ctx))
            .catch_unwind()
            .await
        {
            Ok(Ok(value)) => on_success(tx, value).await,
            Ok(Err(error)) => on_failure(tx, error).await,
            Err(panic) => {
                let detail = panic_detail(panic_context, panic.as_ref());
                on_failure(tx, crate::error::TerminalError::Operation(detail)).await;
            }
        }
    });
}

fn spawn_handoff_workflow_callback<T, Fut, F>(
    ctx: Arc<IoContext>,
    tx: UiUpdateSender,
    operation: WorkflowHandoffOperationOwner,
    spec: WorkflowHandoffSpec,
    workflow: F,
) where
    T: Clone + Send + 'static,
    Fut: Future<Output = WorkflowTerminalOutcome<T>> + Send + 'static,
    F: FnOnce(
            Arc<RwLock<aura_app::ui::types::AppCore>>,
            Option<aura_app::ui_contract::OperationInstanceId>,
        ) -> Fut
        + Send
        + 'static,
{
    spawn_handoff_workflow_callback_with_success(
        ctx,
        tx,
        operation,
        spec,
        workflow,
        |_tx, _value| async {},
    );
}

fn spawn_handoff_workflow_callback_with_success<T, Fut, F, Success, SuccessFut>(
    ctx: Arc<IoContext>,
    tx: UiUpdateSender,
    operation: WorkflowHandoffOperationOwner,
    spec: WorkflowHandoffSpec,
    workflow: F,
    on_success: Success,
) where
    T: Clone + Send + 'static,
    Fut: Future<Output = WorkflowTerminalOutcome<T>> + Send + 'static,
    F: FnOnce(
            Arc<RwLock<aura_app::ui::types::AppCore>>,
            Option<aura_app::ui_contract::OperationInstanceId>,
        ) -> Fut
        + Send
        + 'static,
    Success: FnOnce(UiUpdateSender, T) -> SuccessFut + Send + 'static,
    SuccessFut: Future<Output = ()> + Send + 'static,
{
    let app_core = ctx.app_core_raw().clone();
    let spec_for_handoff = spec.clone();
    spawn_ctx(ctx, async move {
        let workflow_instance_id = operation.workflow_instance_id();
        let transfer = operation.handoff_to_app_workflow(spec_for_handoff.scope);

        match transfer
            .run_workflow(
                app_core.clone(),
                tx.clone(),
                spec.panic_context,
                workflow(app_core.clone(), workflow_instance_id),
            )
            .await
        {
            Ok(value) => {
                on_success(tx.clone(), value).await;
            }
            Err(aura_app::frontend_primitives::SubmittedOperationWorkflowError::Workflow(
                error,
            )) => {
                emit_error_toast(
                    &tx,
                    spec.toast_scope,
                    format!("{}: {error}", spec.failure_prefix),
                )
                .await;
            }
            Err(
                aura_app::frontend_primitives::SubmittedOperationWorkflowError::Protocol(detail)
                | aura_app::frontend_primitives::SubmittedOperationWorkflowError::Panicked(detail),
            ) => {
                emit_error_toast(&tx, spec.toast_scope, detail).await;
            }
        }
    });
}

fn invitation_import_success_updates(code: &str) -> Vec<UiUpdate> {
    vec![UiUpdate::InvitationImported {
        invitation_code: code.to_string(),
    }]
}

async fn run_invitation_import_flow(
    ctx: Arc<IoContext>,
    tx: UiUpdateSender,
    code: String,
    operation: WorkflowHandoffOperationOwner,
) {
    let app_core = ctx.app_core_raw().clone();
    let operation_id = OperationId::invitation_accept();
    let kind = SemanticOperationKind::AcceptContactInvitation;
    let operation_instance_id = operation.harness_handle().instance_id().clone();
    let workflow_instance_id = operation.workflow_instance_id();
    let transfer =
        operation.handoff_to_app_workflow(SemanticOperationTransferScope::InvitationImport);

    let invitation = match import_invitation_details(&app_core, &code).await {
        Ok(invitation) => invitation,
        Err(error) => {
            tracing::error!(error = %error, "import_invitation_details failed");
            let _ = apply_handed_off_terminal_status(
                &app_core,
                &tx,
                operation_id,
                operation_instance_id,
                kind,
                Some(aura_app::ui_contract::WorkflowTerminalStatus {
                    causality: None,
                    status: SemanticOperationStatus::failed(
                        kind,
                        SemanticOperationError::new(
                            SemanticFailureDomain::Command,
                            SemanticFailureCode::InternalError,
                        )
                        .with_detail(error.to_string()),
                    ),
                }),
            )
            .await;
            send_ui_update_required(
                &tx,
                UiUpdate::ToastAdded(ToastMessage::error(
                    "invitation",
                    format!("Import invitation failed: {error}"),
                )),
            )
            .await;
            return;
        }
    };

    match transfer
        .run_workflow(
            app_core.clone(),
            tx.clone(),
            "accept_imported_invitation",
            aura_app::ui::workflows::invitation::handoff::accept_imported_invitation(
                &app_core,
                aura_app::ui::workflows::invitation::handoff::AcceptImportedInvitationRequest {
                    invitation,
                    operation_instance_id: workflow_instance_id,
                },
            ),
        )
        .await
    {
        Ok(()) => {
            for update in invitation_import_success_updates(&code) {
                send_ui_update_required(&tx, update).await;
            }
        }
        Err(aura_app::frontend_primitives::SubmittedOperationWorkflowError::Workflow(error)) => {
            emit_error_toast(
                &tx,
                "invitation",
                format!("Import invitation failed: {error}"),
            )
            .await;
        }
        Err(
            aura_app::frontend_primitives::SubmittedOperationWorkflowError::Protocol(detail)
            | aura_app::frontend_primitives::SubmittedOperationWorkflowError::Panicked(detail),
        ) => {
            emit_error_toast(&tx, "invitation", detail).await;
        }
    }
}

fn enqueue_invalid_lan_authority_toast(
    ctx: Arc<IoContext>,
    tx: UiUpdateSender,
    authority_id: String,
    error: String,
) {
    enqueue_ui_update_required(
        ctx,
        tx,
        UiUpdate::ToastAdded(ToastMessage::error(
            "lan",
            format!("Invalid authority id '{authority_id}': {error}"),
        )),
    );
}

// ---------------------------------------------------------------------------
// Command outcome classification
// ---------------------------------------------------------------------------

type CommandOutcomeStatus = CommandTerminalOutcomeStatus;
type CommandReasonCode = CommandTerminalReasonCode;

fn classify_command_error(
    error: &aura_core::AuraError,
) -> (CommandOutcomeStatus, CommandReasonCode) {
    let classification = classify_terminal_execution_error(error);
    (classification.status, classification.reason)
}

fn classify_chat_command_error(
    error: &aura_app::ui::workflows::chat_commands::CommandError,
) -> (CommandOutcomeStatus, CommandReasonCode) {
    use aura_app::ui::workflows::chat_commands::CommandError;

    match error {
        CommandError::NotACommand => (
            CommandOutcomeStatus::Invalid,
            CommandReasonCode::InvalidArgument,
        ),
        CommandError::UnknownCommand(_) => {
            (CommandOutcomeStatus::Invalid, CommandReasonCode::NotFound)
        }
        CommandError::MissingArgument { .. } | CommandError::InvalidArgument { .. } => (
            CommandOutcomeStatus::Invalid,
            CommandReasonCode::InvalidArgument,
        ),
    }
}

fn classify_command_resolver_error(
    error: &aura_app::ui::workflows::strong_command::CommandResolverError,
) -> (CommandOutcomeStatus, CommandReasonCode) {
    use aura_app::ui::workflows::strong_command::CommandResolverError;

    match error {
        CommandResolverError::UnknownTarget { .. } => {
            (CommandOutcomeStatus::Invalid, CommandReasonCode::NotFound)
        }
        CommandResolverError::AmbiguousTarget { .. } => (
            CommandOutcomeStatus::Invalid,
            CommandReasonCode::InvalidArgument,
        ),
        CommandResolverError::StaleSnapshot { .. } => (
            CommandOutcomeStatus::Failed,
            CommandReasonCode::InvalidState,
        ),
        CommandResolverError::ParseError { .. } => (
            CommandOutcomeStatus::Invalid,
            CommandReasonCode::InvalidArgument,
        ),
        CommandResolverError::MissingCurrentChannel { .. } => (
            CommandOutcomeStatus::Invalid,
            CommandReasonCode::MissingActiveContext,
        ),
    }
}

fn command_outcome_message(
    message: impl Into<String>,
    status: CommandOutcomeStatus,
    reason: CommandReasonCode,
    consistency: Option<&str>,
) -> String {
    let metadata = format!(
        "[s={} r={} c={}]",
        status.as_str(),
        reason.as_str(),
        consistency.unwrap_or("none")
    );
    let message = message.into();
    if message.is_empty() {
        metadata
    } else {
        format!("{metadata} {message}")
    }
}

// ---------------------------------------------------------------------------
// App Callbacks (Global)
// ---------------------------------------------------------------------------

/// Global app callbacks (account setup, etc)
#[derive(Clone)]
pub struct AppCallbacks {
    pub(crate) on_create_account: CreateAccountCallback,
    pub(crate) on_import_device_enrollment_during_onboarding: ImportDeviceEnrollmentCallback,
}

impl AppCallbacks {
    #[must_use]
    pub fn new(runtime: &CallbackFactoryRuntime) -> Self {
        let ctx = runtime.ctx();
        let tx = runtime.tx();
        Self {
            on_create_account: Self::make_create_account(ctx.clone(), tx.clone()),
            on_import_device_enrollment_during_onboarding:
                Self::make_import_device_enrollment_during_onboarding(ctx, tx),
        }
    }

    fn make_create_account(ctx: Arc<IoContext>, tx: UiUpdateSender) -> CreateAccountCallback {
        Arc::new(
            move |nickname_suggestion: String, operation: LocalTerminalOperationOwner| {
                let create_nickname = nickname_suggestion.clone();
                spawn_local_terminal_result_callback(
                    ctx.clone(),
                    tx.clone(),
                    operation,
                    "CreateAccount callback",
                    move |ctx| async move { ctx.create_account(&create_nickname).await },
                    move |tx, ()| async move {
                        tracing::info!("tui create_account callback succeeded");
                        send_ui_update_reliable(
                            &tx,
                            UiUpdate::NicknameSuggestionChanged(nickname_suggestion),
                        )
                        .await;
                        send_ui_update_reliable(&tx, UiUpdate::AccountCreated).await;
                    },
                    |tx, error| async move {
                        tracing::error!("tui create_account callback failed: {error}");
                        send_ui_update_reliable(
                            &tx,
                            UiUpdate::operation_failed(UiOperation::CreateAccount, error),
                        )
                        .await;
                    },
                );
            },
        )
    }

    fn make_import_device_enrollment_during_onboarding(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
    ) -> ImportDeviceEnrollmentCallback {
        Arc::new(
            move |code: String, operation: LocalTerminalOperationOwner| {
                spawn_local_terminal_result_callback(
                    ctx.clone(),
                    tx.clone(),
                    operation,
                    "ImportDeviceEnrollmentDuringOnboarding callback",
                    move |ctx| async move { ctx.import_device_enrollment_code(&code).await },
                    |tx, ()| async move {
                        send_ui_update_reliable(&tx, UiUpdate::AccountCreated).await;
                        send_ui_update_reliable(
                            &tx,
                            UiUpdate::ToastAdded(ToastMessage::success(
                                "devices",
                                "Device enrollment invitation accepted",
                            )),
                        )
                        .await;
                    },
                    |tx, error| async move {
                        send_ui_update_reliable(
                            &tx,
                            UiUpdate::operation_failed(
                                UiOperation::ImportDeviceEnrollmentCode,
                                error,
                            ),
                        )
                        .await;
                    },
                );
            },
        )
    }
}

// ---------------------------------------------------------------------------
// All Callbacks Registry
// ---------------------------------------------------------------------------

/// Registry containing all domain callbacks
#[derive(Clone)]
pub struct CallbackRegistry {
    pub chat: ChatCallbacks,
    pub contacts: ContactsCallbacks,
    pub invitations: InvitationsCallbacks,
    pub recovery: RecoveryCallbacks,
    pub settings: SettingsCallbacks,
    pub neighborhood: NeighborhoodCallbacks,
    pub app: AppCallbacks,
}

impl CallbackRegistry {
    /// Create all callbacks from context
    pub fn new(ctx: Arc<IoContext>, tx: UiUpdateSender) -> Self {
        let runtime = CallbackFactoryRuntime::new(ctx, tx);
        Self {
            chat: ChatCallbacks::new(&runtime),
            contacts: ContactsCallbacks::new(&runtime),
            invitations: InvitationsCallbacks::new(&runtime),
            recovery: RecoveryCallbacks::new(&runtime),
            settings: SettingsCallbacks::new(&runtime),
            neighborhood: NeighborhoodCallbacks::new(&runtime),
            app: AppCallbacks::new(&runtime),
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn invitation_import_success_updates_emit_import_notice() {
        let updates = invitation_import_success_updates("code-123");

        assert_eq!(updates.len(), 1);
        assert!(matches!(
            updates.first(),
            Some(UiUpdate::InvitationImported { invitation_code })
                if invitation_code == "code-123"
        ));
    }

    #[test]
    fn command_result_classification_uses_upstream_typed_reason_codes() {
        let result = aura_app::ui::workflows::strong_command::CommandExecutionResult {
            consistency_requirement:
                aura_app::ui::workflows::strong_command::ConsistencyRequirement::Enforced,
            completion_outcome:
                aura_app::ui::workflows::strong_command::CommandCompletionOutcome::Degraded {
                    requirement:
                        aura_app::ui::workflows::strong_command::ConsistencyRequirement::Enforced,
                    reason:
                        aura_app::ui::workflows::strong_command::ConsistencyDegradedReason::OperationTimedOut,
                },
            details: None,
        };
        let classification = result
            .terminal_classification()
            .expect("degraded result should classify");

        assert_eq!(classification.status.as_str(), "failed");
        assert_eq!(classification.reason.as_str(), "operation_timed_out");
    }

    #[test]
    fn command_outcome_message_includes_typed_timeout_metadata() {
        let message = command_outcome_message(
            "/invite: consistency barrier timed out (partial-timeout)",
            CommandOutcomeStatus::Failed,
            CommandReasonCode::OperationTimedOut,
            Some("partial-timeout"),
        );

        assert!(message.contains("[s=failed r=operation_timed_out c=partial-timeout]"));
    }
}
