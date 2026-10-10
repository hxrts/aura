//! Invitation domain callbacks.

use super::*;

/// All callbacks for the invitations screen
#[derive(Clone)]
pub struct InvitationsCallbacks {
    pub(crate) on_accept: IdHandoffCallback,
    pub(crate) on_decline: IdLocalOwnedCallback,
    pub(crate) on_revoke: IdLocalOwnedCallback,
    pub(crate) on_create: CreateInvitationCallback,
    pub on_export: ExportInvitationCallback,
    pub(crate) on_import: ImportInvitationOwnedCallback,
    pub(crate) on_accept_contact_code: IdHandoffCallback,
}

impl InvitationsCallbacks {
    #[must_use]
    pub fn new(runtime: &CallbackFactoryRuntime) -> Self {
        let ctx = runtime.ctx();
        let tx = runtime.tx();
        Self {
            on_accept: Self::make_accept(ctx.clone(), tx.clone()),
            on_decline: Self::make_decline(ctx.clone(), tx.clone()),
            on_revoke: Self::make_revoke(ctx.clone(), tx.clone()),
            on_create: Self::make_create(ctx.clone(), tx.clone()),
            on_export: Self::make_export(ctx.clone(), tx.clone()),
            on_import: Self::make_import(ctx.clone(), tx.clone()),
            on_accept_contact_code: Self::make_accept_contact_code(ctx, tx),
        }
    }

    fn make_accept(ctx: Arc<IoContext>, tx: UiUpdateSender) -> IdHandoffCallback {
        Arc::new(move |invitation_id, operation| {
            let instance_id = operation.harness_handle().instance_id().clone();
            let kind = operation.kind();
            let followup_app_core = ctx.app_core_raw().clone();
            spawn_handoff_workflow_callback_with_success(
                ctx.clone(),
                tx.clone(),
                operation,
                WorkflowHandoffSpec::new(
                    SemanticOperationTransferScope::AcceptInvitation,
                    "invitation",
                    "Accept invitation failed",
                    "accept_invitation_by_id",
                ),
                move |app_core, _| async move {
                    aura_app::ui::workflows::invitation::handoff::accept_invitation_by_id(
                        &app_core,
                        aura_app::ui::workflows::invitation::handoff::AcceptInvitationByIdRequest {
                            invitation_id,
                            operation_instance_id: instance_id,
                            operation_kind: kind,
                        },
                    )
                    .await
                },
                move |tx, invitation| async move {
                    send_ui_update_reliable(
                        &tx,
                        UiUpdate::InvitationAccepted {
                            invitation_id: invitation.invitation_id().to_string(),
                        },
                    )
                    .await;
                    if matches!(
                        invitation.info().invitation_type,
                        aura_app::ui::types::InvitationBridgeType::Contact { .. }
                    ) {
                        aura_app::ui::workflows::invitation::run_post_contact_accept_followups(
                            &followup_app_core,
                            invitation.info().sender_id,
                        )
                        .await;
                    }
                },
            );
        })
    }

    fn make_decline(ctx: Arc<IoContext>, tx: UiUpdateSender) -> IdLocalOwnedCallback {
        Arc::new(
            move |invitation_id: String, operation: LocalTerminalOperationOwner| {
                let inv_id = invitation_id.clone();
                spawn_local_terminal_result_callback(
                    ctx.clone(),
                    tx.clone(),
                    operation,
                    "DeclineInvitation callback",
                    move |ctx| async move {
                        let app_core = ctx.app_core_raw().clone();
                        aura_app::ui::workflows::invitation::decline_invitation_by_str(
                            &app_core,
                            &invitation_id,
                        )
                        .await
                        .map_err(Into::into)
                    },
                    move |tx, ()| async move {
                        send_ui_update_reliable(
                            &tx,
                            UiUpdate::InvitationDeclined {
                                invitation_id: inv_id,
                            },
                        )
                        .await;
                    },
                    |tx, error| async move {
                        emit_error_toast(
                            &tx,
                            "invitation",
                            format!("Decline invitation failed: {error}"),
                        )
                        .await;
                    },
                );
            },
        )
    }

    fn make_revoke(ctx: Arc<IoContext>, tx: UiUpdateSender) -> IdLocalOwnedCallback {
        Arc::new(
            move |invitation_id: String, operation: LocalTerminalOperationOwner| {
                spawn_local_terminal_result_callback(
                    ctx.clone(),
                    tx.clone(),
                    operation,
                    "CancelInvitation callback",
                    move |ctx| async move {
                        let app_core = ctx.app_core_raw().clone();
                        aura_app::ui::workflows::invitation::cancel_invitation_by_str(
                            &app_core,
                            &invitation_id,
                        )
                        .await
                        .map_err(Into::into)
                    },
                    |_tx, ()| async {},
                    |tx, error| async move {
                        emit_error_toast(
                            &tx,
                            "invitation",
                            format!("Revoke invitation failed: {error}"),
                        )
                        .await;
                    },
                );
            },
        )
    }

    fn make_create(ctx: Arc<IoContext>, tx: UiUpdateSender) -> CreateInvitationCallback {
        Arc::new(
            move |receiver_id: Option<AuthorityId>,
                  invitation_type: String,
                  nickname: Option<String>,
                  receiver_nickname: Option<String>,
                  message: Option<String>,
                  ttl_secs: Option<u64>,
                  operation: LocalTerminalOperationOwner| {
                let operation_handle = operation.harness_handle();
                let operation_instance_id = operation_handle.instance_id().clone();
                let operation_id = operation_handle.operation_id().clone();
                let workflow_instance_id = operation_instance_id.clone();
                spawn_local_terminal_result_callback(
                    ctx.clone(),
                    tx.clone(),
                    operation,
                    "CreateInvitation callback",
                    move |ctx| async move {
                        ctx.create_invitation_code(
                            receiver_id,
                            &invitation_type,
                            nickname,
                            receiver_nickname,
                            message,
                            ttl_secs,
                            Some(workflow_instance_id),
                        )
                        .await
                    },
                    |tx, code| async move {
                        if let Err(e) = copy_to_clipboard(&code) {
                            tracing::debug!(error = %e, "clipboard copy failed; code still available in UI");
                        }
                        send_ui_update_reliable(
                            &tx,
                            UiUpdate::InvitationExported {
                                code,
                                operation_id: Some(operation_id),
                                instance_id: Some(operation_instance_id),
                            },
                        )
                        .await;
                    },
                    |tx, error| async move {
                        send_ui_update_reliable(
                            &tx,
                            UiUpdate::ToastAdded(ToastMessage::error(
                                "invitation",
                                format!("Create invitation failed: {error}"),
                            )),
                        )
                        .await;
                    },
                );
            },
        )
    }

    fn make_export(ctx: Arc<IoContext>, tx: UiUpdateSender) -> ExportInvitationCallback {
        Arc::new(move |invitation_id: String| {
            spawn_observed_result_callback(
                ctx.clone(),
                tx.clone(),
                "export invitation callback",
                move |ctx| async move { ctx.export_invitation_code(&invitation_id).await },
                |tx, code| async move {
                    if let Err(e) = copy_to_clipboard(&code) {
                        tracing::debug!(error = %e, "clipboard copy failed; code still available in UI");
                    }
                    send_ui_update_reliable(
                        &tx,
                        UiUpdate::InvitationExported {
                            code,
                            operation_id: None,
                            instance_id: None,
                        },
                    )
                    .await;
                },
                |tx, error| async move {
                    emit_error_toast(
                        &tx,
                        "invitation",
                        format!("Export invitation failed: {error}"),
                    )
                    .await;
                },
            );
        })
    }

    fn make_accept_contact_code(ctx: Arc<IoContext>, tx: UiUpdateSender) -> IdHandoffCallback {
        Arc::new(move |code, operation| {
            let instance_id = operation.harness_handle().instance_id().clone();
            let followup_app_core = ctx.app_core_raw().clone();
            spawn_handoff_workflow_callback_with_success(
                ctx.clone(),
                tx.clone(),
                operation,
                WorkflowHandoffSpec::new(
                    SemanticOperationTransferScope::AcceptInvitation,
                    "invitation",
                    "Accept Contact invitation failed",
                    "accept_contact_invitation_from_code",
                ),
                move |app_core, _| async move {
                    aura_app::ui::workflows::invitation::handoff::accept_contact_invitation_from_code(
                        &app_core,
                        aura_app::ui::workflows::invitation::handoff::AcceptContactInvitationFromCodeRequest {
                            code,
                            operation_instance_id: instance_id,
                        },
                    ).await
                },
                move |tx, invitation| async move {
                    send_ui_update_reliable(
                        &tx,
                        UiUpdate::InvitationAccepted {
                            invitation_id: invitation.invitation_id().to_string(),
                        },
                    )
                    .await;
                    aura_app::ui::workflows::invitation::run_post_contact_accept_followups(
                        &followup_app_core,
                        invitation.info().sender_id,
                    )
                    .await;
                },
            );
        })
    }

    fn make_import(ctx: Arc<IoContext>, tx: UiUpdateSender) -> ImportInvitationOwnedCallback {
        Arc::new(
            move |code: String, operation: WorkflowHandoffOperationOwner| {
                let ctx = ctx.clone();
                let tx = tx.clone();
                spawn_ctx(ctx.clone(), async move {
                    run_invitation_import_flow(ctx, tx, code, operation).await;
                });
            },
        )
    }
}
