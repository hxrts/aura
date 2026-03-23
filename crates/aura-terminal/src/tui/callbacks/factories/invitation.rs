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
}

impl InvitationsCallbacks {
    #[must_use]
    pub fn new(ctx: Arc<IoContext>, tx: UiUpdateSender) -> Self {
        Self {
            on_accept: Self::make_accept(ctx.clone(), tx.clone()),
            on_decline: Self::make_decline(ctx.clone(), tx.clone()),
            on_revoke: Self::make_revoke(ctx.clone(), tx.clone()),
            on_create: Self::make_create(ctx.clone(), tx.clone()),
            on_export: Self::make_export(ctx.clone(), tx.clone()),
            on_import: Self::make_import(ctx, tx),
        }
    }

    fn make_accept(ctx: Arc<IoContext>, tx: UiUpdateSender) -> IdHandoffCallback {
        Arc::new(move |invitation_id: String, operation: WorkflowHandoffOperationOwner| {
            let inv_id = invitation_id.clone();
            let ctx = ctx.clone();
            let tx = tx.clone();
            spawn_ctx(ctx.clone(), async move {
                let app_core = ctx.app_core_raw().clone();
                let operation_instance_id = operation.harness_handle().instance_id().clone();
                let transfer = operation.handoff_to_app_workflow(
                    SemanticOperationTransferScope::AcceptInvitation,
                );
                match aura_app::ui::workflows::invitation::accept_invitation_by_str_with_instance(
                    &app_core,
                    &invitation_id,
                    Some(operation_instance_id.clone()),
                )
                .await
                {
                    Ok(accepted) => {
                        // Terminal settlement first.
                        let terminal = aura_app::ui_contract::WorkflowTerminalStatus {
                            causality: None,
                            status: SemanticOperationStatus::new(
                                transfer.kind(),
                                SemanticOperationPhase::Succeeded,
                            ),
                        };
                        let _ = apply_handed_off_terminal_status(
                            &app_core,
                            &tx,
                            transfer.operation_id().clone(),
                            operation_instance_id,
                            transfer.kind(),
                            Some(terminal),
                        )
                        .await;

                        // Best-effort UI enrichment after terminal settlement.
                        send_ui_update_reliable(
                            &tx,
                            UiUpdate::InvitationAccepted {
                                invitation_id: inv_id.clone(),
                            },
                        )
                        .await;
                        let invitation_kind = match accepted.invitation_type {
                            aura_app::ui::types::InvitationBridgeType::Contact { .. }
                            | aura_app::ui::types::InvitationBridgeType::Channel { .. } => {
                                InvitationFactKind::Contact
                            }
                            aura_app::ui::types::InvitationBridgeType::Guardian { .. }
                            | aura_app::ui::types::InvitationBridgeType::DeviceEnrollment { .. } => {
                                InvitationFactKind::Generic
                            }
                        };
                        send_ui_update_reliable(
                            &tx,
                            UiUpdate::RuntimeFactsUpdated {
                                replace_kinds: vec![RuntimeEventKind::InvitationAccepted],
                                facts: vec![RuntimeFact::InvitationAccepted {
                                    invitation_kind,
                                    authority_id: Some(accepted.sender_id.to_string()),
                                    operation_state: Some(OperationState::Succeeded),
                                }],
                            },
                        )
                        .await;
                    }
                    Err(error) => {
                        // Terminal failure settlement.
                        let terminal = aura_app::ui_contract::WorkflowTerminalStatus {
                            causality: None,
                            status: SemanticOperationStatus::failed(
                                transfer.kind(),
                                SemanticOperationError::new(
                                    SemanticFailureDomain::Command,
                                    SemanticFailureCode::InternalError,
                                )
                                .with_detail(error.to_string()),
                            ),
                        };
                        let _ = apply_handed_off_terminal_status(
                            &app_core,
                            &tx,
                            transfer.operation_id().clone(),
                            operation_instance_id,
                            transfer.kind(),
                            Some(terminal),
                        )
                        .await;
                        emit_error_toast(
                            &tx,
                            "invitation",
                            format!("Accept invitation failed: {error}"),
                        )
                        .await;
                    }
                }
            });
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
                        ctx.dispatch(EffectCommand::DeclineInvitation { invitation_id })
                            .await
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
                        ctx.dispatch(EffectCommand::CancelInvitation { invitation_id })
                            .await
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
            move |receiver_id: AuthorityId,
                  invitation_type: String,
                  message: Option<String>,
                  ttl_secs: Option<u64>,
                  operation: LocalTerminalOperationOwner| {
                let operation_instance_id = operation.harness_handle().instance_id().clone();
                spawn_local_terminal_result_callback(
                    ctx.clone(),
                    tx.clone(),
                    operation,
                    "CreateInvitation callback",
                    move |ctx| async move {
                        ctx.create_invitation_code(
                            receiver_id,
                            &invitation_type,
                            message,
                            ttl_secs,
                            Some(operation_instance_id),
                        )
                        .await
                    },
                    |tx, code| async move {
                        if let Err(e) = copy_to_clipboard(&code) {
                            tracing::debug!(error = %e, "clipboard copy failed; code still available in UI");
                        }
                        send_ui_update_reliable(&tx, UiUpdate::InvitationExported { code }).await;
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
                    send_ui_update_reliable(&tx, UiUpdate::InvitationExported { code }).await;
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
