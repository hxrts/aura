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
            let ctx = ctx.clone();
            let tx = tx.clone();
            let inv_id = invitation_id.clone();
            let cmd = EffectCommand::AcceptInvitation { invitation_id };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch_with_response(cmd).await {
                    Ok(OpResponse::InvitationAccepted {
                        sender_id,
                        invitation_type,
                        ..
                    }) => {
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
                    }
                    Ok(_) => {}
                    Err(_error) => {}
                }
            });
        })
    }

    fn make_decline(ctx: Arc<IoContext>, tx: UiUpdateSender) -> InvitationCallback {
        Arc::new(move |invitation_id: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let inv_id = invitation_id.clone();
            let cmd = EffectCommand::DeclineInvitation { invitation_id };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_reliable(
                            &tx,
                            UiUpdate::InvitationDeclined {
                                invitation_id: inv_id,
                            },
                        )
                        .await;
                    }
                    Err(_e) => {
                        tracing::debug!(error = %_e, "dispatch error (surfaced via ERROR_SIGNAL)");
                    }
                }
            });
        })
    }

    fn make_revoke(ctx: Arc<IoContext>, _tx: UiUpdateSender) -> InvitationCallback {
        Arc::new(move |invitation_id: String| {
            let ctx = ctx.clone();
            let cmd = EffectCommand::CancelInvitation { invitation_id };
            spawn_ctx(ctx.clone(), async move {
                let _ = ctx.dispatch(cmd).await;
            });
        })
    }

    fn make_create(ctx: Arc<IoContext>, tx: UiUpdateSender) -> CreateInvitationCallback {
        Arc::new(
            move |receiver_id: AuthorityId,
                  invitation_type: String,
                  message: Option<String>,
                  ttl_secs: Option<u64>,
                  operation: Option<LocalTerminalOperationOwner>| {
                let ctx = ctx.clone();
                let tx = tx.clone();
                spawn_ctx(ctx.clone(), async move {
                    let result = ctx
                        .create_invitation_code(receiver_id, &invitation_type, message, ttl_secs)
                        .await;
                    match result {
                        Ok(code) => {
                            if let Some(operation) = operation {
                                operation.succeed().await;
                            }
                            if let Err(e) = copy_to_clipboard(&code) {
                                tracing::debug!(error = %e, "clipboard copy failed; code still available in UI");
                            }
                            send_ui_update_reliable(&tx, UiUpdate::InvitationExported { code })
                                .await;
                        }
                        Err(e) => {
                            if let Some(operation) = operation {
                                operation.fail(e.to_string()).await;
                            }
                            send_ui_update_reliable(
                                &tx,
                                UiUpdate::ToastAdded(ToastMessage::error(
                                    "invitation",
                                    format!("Create invitation failed: {e}"),
                                )),
                            )
                            .await;
                        }
                    }
                });
            },
        )
    }

    fn make_export(ctx: Arc<IoContext>, tx: UiUpdateSender) -> ExportInvitationCallback {
        Arc::new(move |invitation_id: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            spawn_ctx(ctx.clone(), async move {
                match ctx.export_invitation_code(&invitation_id).await {
                    Ok(code) => {
                        if let Err(e) = copy_to_clipboard(&code) {
                            tracing::debug!(error = %e, "clipboard copy failed; code still available in UI");
                        }
                        send_ui_update_reliable(&tx, UiUpdate::InvitationExported { code }).await;
                    }
                    Err(_e) => {
                        tracing::debug!(error = %_e, "dispatch error (surfaced via ERROR_SIGNAL)");
                    }
                }
            });
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
