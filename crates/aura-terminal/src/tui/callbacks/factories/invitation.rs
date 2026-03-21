//! Invitation domain callbacks.

use super::*;

/// All callbacks for the invitations screen
#[derive(Clone)]
pub struct InvitationsCallbacks {
    pub on_accept: InvitationCallback,
    pub on_decline: InvitationCallback,
    pub on_revoke: InvitationCallback,
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

    fn make_accept(ctx: Arc<IoContext>, tx: UiUpdateSender) -> InvitationCallback {
        Arc::new(move |invitation_id: String| {
            let inv_id = invitation_id.clone();
            spawn_observed_result_callback(
                ctx.clone(),
                tx.clone(),
                "accept invitation callback",
                move |ctx| async move {
                    match ctx
                        .dispatch_with_response(EffectCommand::AcceptInvitation { invitation_id })
                        .await?
                    {
                        OpResponse::InvitationAccepted {
                            sender_id,
                            invitation_type,
                            ..
                        } => Ok((sender_id, invitation_type)),
                        _ => Err(crate::error::TerminalError::Operation(
                            "Accept invitation returned unexpected response".to_string(),
                        )),
                    }
                },
                move |tx, (sender_id, invitation_type)| async move {
                    send_ui_update_reliable(
                        &tx,
                        UiUpdate::InvitationAccepted {
                            invitation_id: inv_id.clone(),
                        },
                    )
                    .await;
                    let invitation_kind = if invitation_type.starts_with("contact")
                        || invitation_type.starts_with("channel:")
                    {
                        InvitationFactKind::Contact
                    } else {
                        InvitationFactKind::Generic
                    };
                    send_ui_update_reliable(
                        &tx,
                        UiUpdate::RuntimeFactsUpdated {
                            replace_kinds: vec![RuntimeEventKind::InvitationAccepted],
                            facts: vec![RuntimeFact::InvitationAccepted {
                                invitation_kind,
                                authority_id: Some(sender_id),
                                operation_state: Some(OperationState::Succeeded),
                            }],
                        },
                    )
                    .await;
                },
                |tx, error| async move {
                    emit_error_toast(
                        &tx,
                        "invitation",
                        format!("Accept invitation failed: {error}"),
                    )
                    .await;
                },
            );
        })
    }

    fn make_decline(ctx: Arc<IoContext>, tx: UiUpdateSender) -> InvitationCallback {
        Arc::new(move |invitation_id: String| {
            let inv_id = invitation_id.clone();
            spawn_observed_dispatch_callback(
                ctx.clone(),
                tx.clone(),
                EffectCommand::DeclineInvitation { invitation_id },
                move |tx| async move {
                    send_ui_update_reliable(
                        &tx,
                        UiUpdate::InvitationDeclined {
                            invitation_id: inv_id,
                        },
                    )
                    .await;
                },
                |error| async move {
                    tracing::debug!(error = %error, "dispatch error (surfaced via ERROR_SIGNAL)");
                },
            );
        })
    }

    fn make_revoke(ctx: Arc<IoContext>, tx: UiUpdateSender) -> InvitationCallback {
        Arc::new(move |invitation_id: String| {
            spawn_observed_dispatch_callback(
                ctx.clone(),
                tx.clone(),
                EffectCommand::CancelInvitation { invitation_id },
                |_| async {},
                |error| async move {
                    tracing::debug!(error = %error, "dispatch error (surfaced via ERROR_SIGNAL)");
                },
            );
        })
    }

    fn make_create(ctx: Arc<IoContext>, tx: UiUpdateSender) -> CreateInvitationCallback {
        Arc::new(
            move |receiver_id: AuthorityId,
                  invitation_type: String,
                  message: Option<String>,
                  ttl_secs: Option<u64>,
                  operation: LocalTerminalOperationOwner| {
                spawn_local_terminal_result_callback(
                    ctx.clone(),
                    tx.clone(),
                    operation,
                    "CreateInvitation callback",
                    move |ctx| async move {
                        ctx.create_invitation_code(receiver_id, &invitation_type, message, ttl_secs)
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
