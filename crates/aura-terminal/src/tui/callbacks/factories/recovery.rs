//! Recovery domain callbacks.

use super::*;
use aura_app::ui_contract::{OperationId, SemanticOperationKind};

/// All callbacks for the recovery screen
#[derive(Clone)]
pub struct RecoveryCallbacks {
    pub(crate) on_start_recovery: NoArgLocalOwnedCallback,
    pub(crate) on_select_guardian: GuardianSelectCallback,
    pub(crate) on_submit_approval: IdLocalOwnedCallback,
}

impl RecoveryCallbacks {
    #[must_use]
    pub fn new(runtime: &CallbackFactoryRuntime) -> Self {
        let ctx = runtime.ctx();
        let tx = runtime.tx();
        Self {
            on_start_recovery: Self::make_start_recovery(ctx.clone(), tx.clone()),
            on_select_guardian: Self::make_select_guardian(ctx.clone(), tx.clone()),
            on_submit_approval: Self::make_submit_approval(ctx, tx),
        }
    }

    fn make_start_recovery(ctx: Arc<IoContext>, tx: UiUpdateSender) -> NoArgLocalOwnedCallback {
        Arc::new(move |operation: LocalTerminalOperationOwner| {
            spawn_local_terminal_result_callback(
                ctx.clone(),
                tx.clone(),
                operation,
                "StartRecovery callback",
                move |ctx| async move {
                    let app_core = ctx.app_core_raw().clone();
                    aura_app::ui::workflows::recovery::start_recovery_from_state(&app_core)
                        .await
                        .map(|_| ())
                        .map_err(Into::into)
                },
                |tx, ()| async move {
                    send_ui_update_required(&tx, UiUpdate::RecoveryStarted).await;
                },
                |tx, error| async move {
                    emit_error_toast(&tx, "recovery", format!("Start recovery failed: {error}"))
                        .await;
                },
            );
        })
    }

    fn make_select_guardian(ctx: Arc<IoContext>, tx: UiUpdateSender) -> GuardianSelectCallback {
        Arc::new(
            move |contact_id: String, operation: WorkflowHandoffOperationOwner| {
                let selected_contact_id = contact_id.clone();
                spawn_handoff_workflow_callback_with_success(
                    ctx.clone(),
                    tx.clone(),
                    operation,
                    WorkflowHandoffSpec::new(
                        OperationId::invitation_create(),
                        SemanticOperationKind::CreateGuardianInvitation,
                        SemanticOperationTransferScope::CreateGuardianInvitation,
                        "recovery",
                        "Create guardian invitation failed",
                        "select_guardian callback",
                    ),
                    move |app_core, workflow_instance_id| async move {
                        let failed_terminal =
                            |detail: String| aura_app::ui_contract::WorkflowTerminalOutcome {
                                result: Err(aura_core::AuraError::internal(detail.clone())),
                                terminal: Some(aura_app::ui_contract::WorkflowTerminalStatus {
                                    causality: None,
                                    status: SemanticOperationStatus::failed(
                                        SemanticOperationKind::CreateGuardianInvitation,
                                        SemanticOperationError::new(
                                            SemanticFailureDomain::Command,
                                            SemanticFailureCode::InternalError,
                                        )
                                        .with_detail(detail),
                                    ),
                                }),
                            };

                        let runtime = {
                            let core = app_core.read().await;
                            core.runtime().cloned()
                        };
                        let Some(runtime) = runtime else {
                            return failed_terminal("Runtime bridge not available".to_string());
                        };

                        let receiver = match contact_id.parse::<AuthorityId>() {
                            Ok(receiver) => receiver,
                            Err(_) => {
                                return failed_terminal(format!(
                                    "Invalid contact ID: {contact_id}"
                                ));
                            }
                        };

                        let outcome = aura_app::ui::workflows::invitation::create_guardian_invitation_with_terminal_status(
                            &app_core,
                            receiver,
                            runtime.authority_id(),
                            None,
                            None,
                            workflow_instance_id,
                        )
                        .await;
                        match outcome.result {
                            Ok(_) => aura_app::ui_contract::WorkflowTerminalOutcome {
                                result: Ok(selected_contact_id),
                                terminal: outcome.terminal,
                            },
                            Err(error) => aura_app::ui_contract::WorkflowTerminalOutcome {
                                result: Err(error),
                                terminal: outcome.terminal,
                            },
                        }
                    },
                    |tx, selected_contact_id| async move {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::GuardianSelected {
                                contact_id: selected_contact_id,
                            },
                        )
                        .await;
                    },
                );
            },
        )
    }

    fn make_submit_approval(ctx: Arc<IoContext>, tx: UiUpdateSender) -> IdLocalOwnedCallback {
        Arc::new(
            move |request_id: String, operation: LocalTerminalOperationOwner| {
                let request_id_clone = request_id.clone();
                spawn_local_terminal_result_callback(
                    ctx.clone(),
                    tx.clone(),
                    operation,
                    "SubmitGuardianApproval callback",
                    move |ctx| async move {
                        let app_core = ctx.app_core_raw().clone();
                        aura_app::ui::workflows::recovery::approve_recovery(
                            &app_core,
                            &aura_core::CeremonyId::new(request_id),
                        )
                        .await
                        .map_err(Into::into)
                    },
                    move |tx, ()| async move {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::ApprovalSubmitted {
                                request_id: request_id_clone,
                            },
                        )
                        .await;
                    },
                    |tx, error| async move {
                        emit_error_toast(
                            &tx,
                            "recovery",
                            format!("Submit approval failed: {error}"),
                        )
                        .await;
                    },
                );
            },
        )
    }
}
