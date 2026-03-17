//! Recovery domain callbacks.

use super::*;

/// All callbacks for the recovery screen
#[derive(Clone)]
pub struct RecoveryCallbacks {
    pub on_start_recovery: RecoveryCallback,
    pub on_add_guardian: RecoveryCallback,
    pub on_select_guardian: GuardianSelectCallback,
    pub on_submit_approval: ApprovalCallback,
}

impl RecoveryCallbacks {
    #[must_use]
    pub fn new(ctx: Arc<IoContext>, tx: UiUpdateSender) -> Self {
        Self {
            on_start_recovery: Self::make_start_recovery(ctx.clone(), tx.clone()),
            on_add_guardian: Self::make_add_guardian(ctx.clone(), tx.clone()),
            on_select_guardian: Self::make_select_guardian(ctx.clone(), tx.clone()),
            on_submit_approval: Self::make_submit_approval(ctx, tx),
        }
    }

    fn make_start_recovery(ctx: Arc<IoContext>, tx: UiUpdateSender) -> RecoveryCallback {
        Arc::new(move || {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let cmd = EffectCommand::StartRecovery;
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_required(&tx, UiUpdate::RecoveryStarted).await;
                    }
                    Err(_e) => {
                        tracing::debug!(error = %_e, "dispatch error (surfaced via ERROR_SIGNAL)");
                    }
                }
            });
        })
    }

    fn make_add_guardian(ctx: Arc<IoContext>, tx: UiUpdateSender) -> RecoveryCallback {
        Arc::new(move || {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let cmd = EffectCommand::InviteGuardian { contact_id: None };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::GuardianAdded {
                                contact_id: "unknown".to_string(),
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

    fn make_select_guardian(ctx: Arc<IoContext>, tx: UiUpdateSender) -> GuardianSelectCallback {
        Arc::new(move |contact_id: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let contact_id_clone = contact_id.clone();
            let cmd = EffectCommand::InviteGuardian {
                contact_id: Some(contact_id),
            };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::GuardianSelected {
                                contact_id: contact_id_clone,
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

    fn make_submit_approval(ctx: Arc<IoContext>, tx: UiUpdateSender) -> ApprovalCallback {
        Arc::new(move |request_id: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let request_id_clone = request_id.clone();
            let cmd = EffectCommand::SubmitGuardianApproval {
                guardian_id: request_id,
            };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::ApprovalSubmitted {
                                request_id: request_id_clone,
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
}
