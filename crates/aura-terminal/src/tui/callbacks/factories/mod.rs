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
use crate::tui::effects::{EffectCommand, OpResponse};
use crate::tui::semantic_lifecycle::{
    apply_handed_off_terminal_status, authoritative_operation_status_update,
    LocalTerminalOperationOwner, SemanticOperationTransferScope, WorkflowHandoffOperationOwner,
};
use crate::tui::types::{AccessLevel, MfaPolicy};
use crate::tui::updates::{UiOperation, UiUpdate, UiUpdateSender};
use async_lock::RwLock;
use aura_app::ui::types::InvitationBridgeType;
use aura_app::ui::workflows::invitation::import_invitation_details;
use aura_app::ui::workflows::strong_command::{
    classify_terminal_execution_error, CommandTerminalOutcomeStatus, CommandTerminalReasonCode,
};
use aura_app::ui_contract::{
    ChannelFactKey, InvitationFactKind, OperationId, OperationState, RuntimeEventKind, RuntimeFact,
    SemanticFailureCode, SemanticFailureDomain, SemanticOperationError, SemanticOperationKind,
    SemanticOperationStatus, WorkflowTerminalOutcome,
};
use aura_core::AuthorityId;
use futures::FutureExt;

use super::types::*;

use crate::tui::updates::{send_ui_update_lossy, send_ui_update_required};

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

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
                match std::panic::AssertUnwindSafe(on_success(tx.clone(), value))
                    .catch_unwind()
                    .await
                {
                    Ok(()) => {
                        operation.succeed().await;
                    }
                    Err(panic) => {
                        let error = crate::error::TerminalError::Operation(panic_detail(
                            panic_context,
                            panic.as_ref(),
                        ));
                        operation.fail(error.to_string()).await;
                        on_failure(tx, error).await;
                    }
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

fn spawn_observed_dispatch_callback<Success, SuccessFut, Failure, FailureFut>(
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
    operation_id: OperationId,
    kind: SemanticOperationKind,
    scope: SemanticOperationTransferScope,
    toast_scope: &'static str,
    failure_prefix: &'static str,
    panic_context: &'static str,
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
        operation_id,
        kind,
        scope,
        toast_scope,
        failure_prefix,
        panic_context,
        workflow,
        |_tx, _value| async {},
    );
}

fn spawn_handoff_workflow_callback_with_success<T, Fut, F, Success, SuccessFut>(
    ctx: Arc<IoContext>,
    tx: UiUpdateSender,
    operation: WorkflowHandoffOperationOwner,
    operation_id: OperationId,
    kind: SemanticOperationKind,
    scope: SemanticOperationTransferScope,
    toast_scope: &'static str,
    failure_prefix: &'static str,
    panic_context: &'static str,
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
    let operation_instance_id = operation.harness_handle().instance_id().clone();
    spawn_ctx(ctx, async move {
        let workflow_instance_id = operation.workflow_instance_id();
        let _ = operation.handoff_to_app_workflow(scope);

        match std::panic::AssertUnwindSafe(workflow(app_core.clone(), workflow_instance_id))
            .catch_unwind()
            .await
        {
            Ok(WorkflowTerminalOutcome {
                result: Ok(value),
                terminal,
            }) => {
                on_success(tx.clone(), value).await;
                if let Err(error) = apply_handed_off_terminal_status(
                    &app_core,
                    &tx,
                    operation_id,
                    operation_instance_id,
                    kind,
                    terminal,
                )
                .await
                {
                    emit_error_toast(&tx, toast_scope, error).await;
                }
            }
            Ok(WorkflowTerminalOutcome {
                result: Err(error),
                terminal,
            }) => {
                let _ = apply_handed_off_terminal_status(
                    &app_core,
                    &tx,
                    operation_id,
                    operation_instance_id.clone(),
                    kind,
                    terminal,
                )
                .await;
                emit_error_toast(&tx, toast_scope, format!("{failure_prefix}: {error}")).await;
            }
            Err(panic) => {
                let detail = panic_detail(panic_context, panic.as_ref());
                let _ = apply_handed_off_terminal_status(
                    &app_core,
                    &tx,
                    operation_id,
                    operation_instance_id,
                    kind,
                    None,
                )
                .await;
                emit_error_toast(&tx, toast_scope, detail).await;
            }
        }
    });
}

fn invitation_import_runtime_fact_update(
    invitation: Option<&aura_app::ui::types::InvitationInfo>,
) -> Option<UiUpdate> {
    let invitation = invitation?;
    if matches!(
        invitation.invitation_type,
        InvitationBridgeType::Contact { .. }
    ) {
        Some(UiUpdate::RuntimeFactsUpdated {
            replace_kinds: vec![RuntimeEventKind::InvitationAccepted],
            facts: vec![RuntimeFact::InvitationAccepted {
                invitation_kind: InvitationFactKind::Contact,
                authority_id: Some(invitation.sender_id.to_string()),
                operation_state: Some(OperationState::Succeeded),
            }],
        })
    } else {
        None
    }
}

fn invitation_import_success_updates(
    code: &str,
    invitation: Option<&aura_app::ui::types::InvitationInfo>,
) -> Vec<UiUpdate> {
    let mut updates = vec![UiUpdate::InvitationImported {
        invitation_code: code.to_string(),
    }];
    if let Some(update) = invitation_import_runtime_fact_update(invitation) {
        updates.push(update);
    }
    updates
}

async fn run_invitation_import_flow(
    ctx: Arc<IoContext>,
    tx: UiUpdateSender,
    code: String,
    operation: WorkflowHandoffOperationOwner,
) {
    let transfer =
        operation.handoff_to_app_workflow(SemanticOperationTransferScope::InvitationImport);

    let app_core = ctx.app_core_raw().clone();
    let invitation = import_invitation_details(&app_core, &code).await.ok();
    match ctx
        .dispatch(EffectCommand::ImportInvitation { code: code.clone() })
        .await
    {
        Ok(_) => {
            for update in invitation_import_success_updates(
                &code,
                invitation.as_ref().map(|handle| handle.info()),
            ) {
                send_ui_update_required(&tx, update).await;
            }
        }
        Err(error) => {
            tracing::error!(error = %error, "ImportInvitation dispatch failed");
            send_ui_update_required(
                &tx,
                authoritative_operation_status_update(
                    transfer.operation_id().clone(),
                    Some(transfer.instance_id().clone()),
                    None,
                    SemanticOperationStatus::failed(
                        transfer.kind(),
                        SemanticOperationError::new(
                            SemanticFailureDomain::Command,
                            SemanticFailureCode::InternalError,
                        )
                        .with_detail(error.to_string()),
                    ),
                ),
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
    pub fn new(ctx: Arc<IoContext>, tx: UiUpdateSender) -> Self {
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
    pub fn new(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
        app_core: Arc<async_lock::RwLock<aura_app::ui::types::AppCore>>,
    ) -> Self {
        Self {
            chat: ChatCallbacks::new(ctx.clone(), tx.clone(), app_core),
            contacts: ContactsCallbacks::new(ctx.clone(), tx.clone()),
            invitations: InvitationsCallbacks::new(ctx.clone(), tx.clone()),
            recovery: RecoveryCallbacks::new(ctx.clone(), tx.clone()),
            settings: SettingsCallbacks::new(ctx.clone(), tx.clone()),
            neighborhood: NeighborhoodCallbacks::new(ctx.clone(), tx.clone()),
            app: AppCallbacks::new(ctx, tx),
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use aura_app::ui::types::{InvitationBridgeStatus, InvitationInfo};

    fn authority(value: &str) -> AuthorityId {
        value.parse().expect("valid authority id")
    }

    fn contact_invitation() -> InvitationInfo {
        InvitationInfo {
            invitation_id: "inv-contact".into(),
            sender_id: authority("authority-00000000-0000-0000-0000-000000000001"),
            receiver_id: authority("authority-00000000-0000-0000-0000-000000000002"),
            invitation_type: InvitationBridgeType::Contact { nickname: None },
            status: InvitationBridgeStatus::Pending,
            created_at_ms: 0,
            expires_at_ms: None,
            message: None,
        }
    }

    fn channel_invitation() -> InvitationInfo {
        InvitationInfo {
            invitation_id: "inv-channel".into(),
            sender_id: authority("authority-00000000-0000-0000-0000-000000000001"),
            receiver_id: authority("authority-00000000-0000-0000-0000-000000000002"),
            invitation_type: InvitationBridgeType::Channel {
                home_id: "home-test".to_string(),
                context_id: None,
                nickname_suggestion: None,
            },
            status: InvitationBridgeStatus::Pending,
            created_at_ms: 0,
            expires_at_ms: None,
            message: None,
        }
    }

    #[test]
    fn invitation_import_success_updates_emit_import_before_runtime_fact() {
        let updates = invitation_import_success_updates("code-123", Some(&contact_invitation()));

        assert!(matches!(
            updates.first(),
            Some(UiUpdate::InvitationImported { invitation_code })
                if invitation_code == "code-123"
        ));
        assert!(matches!(
            updates.get(1),
            Some(UiUpdate::RuntimeFactsUpdated { .. })
        ));
    }

    #[test]
    fn invitation_import_success_updates_skip_runtime_fact_for_non_contact_invites() {
        let updates = invitation_import_success_updates("code-456", Some(&channel_invitation()));

        assert_eq!(updates.len(), 1);
        assert!(matches!(
            updates.first(),
            Some(UiUpdate::InvitationImported { invitation_code })
                if invitation_code == "code-456"
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
