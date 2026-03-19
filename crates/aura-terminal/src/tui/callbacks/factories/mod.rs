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
    authoritative_operation_status_update, LocalTerminalOperationOwner,
    SemanticOperationTransferScope, WorkflowHandoffOperationOwner,
};
use crate::tui::types::{AccessLevel, MfaPolicy};
use crate::tui::updates::{UiOperation, UiUpdate, UiUpdateSender};
use aura_app::ui::types::InvitationBridgeType;
use aura_app::ui::workflows::invitation::import_invitation_details;
use aura_app::ui_contract::{
    ChannelFactKey, InvitationFactKind, OperationState, RuntimeEventKind, RuntimeFact,
    SemanticFailureCode, SemanticFailureDomain, SemanticOperationError, SemanticOperationStatus,
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

#[derive(Clone, Copy)]
enum CommandOutcomeStatus {
    Ok,
    Denied,
    Invalid,
    Failed,
}

impl CommandOutcomeStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Denied => "denied",
            Self::Invalid => "invalid",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Copy)]
enum CommandReasonCode {
    None,
    MissingActiveContext,
    PermissionDenied,
    NotMember,
    NotFound,
    InvalidArgument,
    InvalidState,
    Muted,
    Banned,
    Internal,
}

impl CommandReasonCode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::MissingActiveContext => "missing_active_context",
            Self::PermissionDenied => "permission_denied",
            Self::NotMember => "not_member",
            Self::NotFound => "not_found",
            Self::InvalidArgument => "invalid_argument",
            Self::InvalidState => "invalid_state",
            Self::Muted => "muted",
            Self::Banned => "banned",
            Self::Internal => "internal",
        }
    }
}

fn classify_command_error(
    error: &aura_core::AuraError,
) -> (CommandOutcomeStatus, CommandReasonCode) {
    use aura_core::AuraError;

    // Primary classification: match on the typed error variant.
    match error {
        AuraError::Invalid { .. } => {
            let msg = error.to_string().to_ascii_lowercase();
            if msg.contains("no active home")
                || msg.contains("missing current channel")
                || msg.contains("missing channel scope")
            {
                (
                    CommandOutcomeStatus::Invalid,
                    CommandReasonCode::MissingActiveContext,
                )
            } else if msg.contains("parse error") || msg.contains("missing required argument") {
                (
                    CommandOutcomeStatus::Invalid,
                    CommandReasonCode::InvalidArgument,
                )
            } else if msg.contains("stale snapshot") || msg.contains("invalid state") {
                (
                    CommandOutcomeStatus::Failed,
                    CommandReasonCode::InvalidState,
                )
            } else {
                (
                    CommandOutcomeStatus::Invalid,
                    CommandReasonCode::InvalidArgument,
                )
            }
        }
        AuraError::NotFound { .. } => (CommandOutcomeStatus::Invalid, CommandReasonCode::NotFound),
        AuraError::PermissionDenied { .. } => {
            let msg = error.to_string().to_ascii_lowercase();
            if msg.contains("not a member") {
                (CommandOutcomeStatus::Denied, CommandReasonCode::NotMember)
            } else if msg.contains("muted") {
                (CommandOutcomeStatus::Denied, CommandReasonCode::Muted)
            } else if msg.contains("banned") || msg.contains("ban ") {
                (CommandOutcomeStatus::Denied, CommandReasonCode::Banned)
            } else {
                (
                    CommandOutcomeStatus::Denied,
                    CommandReasonCode::PermissionDenied,
                )
            }
        }
        _ => {
            // Fallback: string match for errors that don't use typed variants yet.
            let msg = error.to_string().to_ascii_lowercase();
            if msg.contains("permission denied") || msg.contains("only moderators") {
                (
                    CommandOutcomeStatus::Denied,
                    CommandReasonCode::PermissionDenied,
                )
            } else if msg.contains("not found") || msg.contains("unknown") {
                (CommandOutcomeStatus::Invalid, CommandReasonCode::NotFound)
            } else if msg.contains("muted") {
                (CommandOutcomeStatus::Denied, CommandReasonCode::Muted)
            } else if msg.contains("banned") {
                (CommandOutcomeStatus::Denied, CommandReasonCode::Banned)
            } else {
                (CommandOutcomeStatus::Failed, CommandReasonCode::Internal)
            }
        }
    }
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
    pub on_import_device_enrollment_during_onboarding: ImportDeviceEnrollmentCallback,
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
                let ctx = ctx.clone();
                let tx = tx.clone();
                spawn_ctx(ctx.clone(), async move {
                    let account_result = std::panic::AssertUnwindSafe(async {
                        ctx.create_account(&nickname_suggestion).await
                    })
                    .catch_unwind()
                    .await;

                    match account_result {
                        Ok(Ok(())) => {
                            tracing::info!("tui create_account callback succeeded");
                            send_ui_update_reliable(
                                &tx,
                                UiUpdate::NicknameSuggestionChanged(nickname_suggestion.clone()),
                            )
                            .await;
                            operation.succeed().await;
                            send_ui_update_reliable(&tx, UiUpdate::AccountCreated).await;
                        }
                        Ok(Err(e)) => {
                            tracing::error!("tui create_account callback failed: {e}");
                            operation.fail(e.to_string()).await;
                            send_ui_update_reliable(
                                &tx,
                                UiUpdate::operation_failed(UiOperation::CreateAccount, e),
                            )
                            .await;
                        }
                        Err(_) => {
                            tracing::error!("tui create_account callback panicked");
                            operation.fail("panic in CreateAccount callback").await;
                            send_ui_update_reliable(
                                &tx,
                                UiUpdate::operation_failed(
                                    UiOperation::CreateAccount,
                                    crate::error::TerminalError::Operation(
                                        "panic in CreateAccount callback".to_string(),
                                    ),
                                ),
                            )
                            .await;
                        }
                    }
                });
            },
        )
    }

    fn make_import_device_enrollment_during_onboarding(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
    ) -> ImportDeviceEnrollmentCallback {
        Arc::new(move |code: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            spawn_ctx(ctx.clone(), async move {
                match ctx.import_device_enrollment_code(&code).await {
                    Ok(()) => {
                        send_ui_update_reliable(&tx, UiUpdate::AccountCreated).await;
                        send_ui_update_reliable(
                            &tx,
                            UiUpdate::ToastAdded(ToastMessage::success(
                                "devices",
                                "Device enrollment invitation accepted",
                            )),
                        )
                        .await;
                    }
                    Err(e) => {
                        send_ui_update_reliable(
                            &tx,
                            UiUpdate::operation_failed(UiOperation::ImportDeviceEnrollmentCode, e),
                        )
                        .await;
                    }
                }
            });
        })
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
}
