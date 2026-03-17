//! Settings and neighborhood domain callbacks.

use super::*;
use std::time::Duration;

use aura_core::effects::time::PhysicalTimeEffects;
use aura_effects::time::PhysicalTimeHandler;

async fn physical_sleep(duration: Duration) {
    let _ = PhysicalTimeHandler::new()
        .sleep_ms(duration.as_millis() as u64)
        .await;
}

/// All callbacks for the settings screen
#[derive(Clone)]
pub struct SettingsCallbacks {
    pub on_update_mfa: Arc<dyn Fn(MfaPolicy) + Send + Sync>,
    pub on_update_nickname_suggestion: UpdateNicknameSuggestionCallback,
    pub on_update_threshold: UpdateThresholdCallback,
    pub on_add_device: AddDeviceCallback,
    pub on_remove_device: RemoveDeviceCallback,
    pub on_import_device_enrollment_on_mobile: ImportDeviceEnrollmentCallback,
}

impl SettingsCallbacks {
    #[must_use]
    pub fn new(ctx: Arc<IoContext>, tx: UiUpdateSender) -> Self {
        Self {
            on_update_mfa: Self::make_update_mfa(ctx.clone(), tx.clone()),
            on_update_nickname_suggestion: Self::make_update_nickname_suggestion(
                ctx.clone(),
                tx.clone(),
            ),
            on_update_threshold: Self::make_update_threshold(ctx.clone(), tx.clone()),
            on_add_device: Self::make_add_device(ctx.clone(), tx.clone()),
            on_remove_device: Self::make_remove_device(ctx.clone(), tx.clone()),
            on_import_device_enrollment_on_mobile: Self::make_import_device_enrollment(ctx, tx),
        }
    }

    fn make_update_mfa(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
    ) -> Arc<dyn Fn(MfaPolicy) + Send + Sync> {
        Arc::new(move |policy: MfaPolicy| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let cmd = EffectCommand::UpdateMfaPolicy {
                require_mfa: policy.requires_mfa(),
            };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_required(&tx, UiUpdate::MfaPolicyChanged(policy)).await;
                    }
                    Err(_e) => {
                        tracing::debug!(error = %_e, "dispatch error (surfaced via ERROR_SIGNAL)");
                    }
                }
            });
        })
    }

    fn make_update_nickname_suggestion(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
    ) -> UpdateNicknameSuggestionCallback {
        Arc::new(move |name: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let name_clone = name.clone();
            let cmd = EffectCommand::UpdateNickname { name };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::NicknameSuggestionChanged(name_clone),
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

    fn make_update_threshold(ctx: Arc<IoContext>, tx: UiUpdateSender) -> UpdateThresholdCallback {
        Arc::new(move |threshold_k: u8, threshold_n: u8| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let config = match crate::tui::effects::ThresholdConfig::new(threshold_k, threshold_n) {
                Ok(config) => config,
                Err(error) => {
                    enqueue_ui_update_required(
                        ctx.clone(),
                        tx.clone(),
                        UiUpdate::operation_failed(
                            UiOperation::UpdateThreshold,
                            crate::error::TerminalError::Input(error),
                        ),
                    );
                    return;
                }
            };
            let cmd = EffectCommand::UpdateThreshold { config };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::ThresholdChanged {
                                k: threshold_k,
                                n: threshold_n,
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

    fn make_add_device(ctx: Arc<IoContext>, tx: UiUpdateSender) -> AddDeviceCallback {
        Arc::new(
            move |nickname_suggestion: String, invitee_authority_id: Option<AuthorityId>| {
                let ctx = ctx.clone();
                let tx = tx.clone();
                spawn_ctx(ctx.clone(), async move {
                    let start = match ctx
                        .start_device_enrollment(&nickname_suggestion, invitee_authority_id)
                        .await
                    {
                        Ok(start) => start,
                        Err(error) => {
                            send_ui_update_reliable(
                                &tx,
                                UiUpdate::ToastAdded(ToastMessage::error(
                                    "devices",
                                    format!("Start device enrollment failed: {error}"),
                                )),
                            )
                            .await;
                            return;
                        }
                    };

                    send_ui_update_reliable(
                        &tx,
                        UiUpdate::DeviceEnrollmentStarted {
                            ceremony_id: start.ceremony_id.clone(),
                            nickname_suggestion: nickname_suggestion.clone(),
                            enrollment_code: start.enrollment_code.clone(),
                            pending_epoch: start.pending_epoch,
                            device_id: start.device_id.clone(),
                        },
                    )
                    .await;

                    // Prime status quickly (best-effort) so the modal has counters immediately.
                    let ceremony_id_typed = CeremonyId::new(start.ceremony_id.clone());
                    if let Ok(status) =
                        aura_app::ui::workflows::ceremonies::get_key_rotation_ceremony_status(
                            ctx.app_core_raw(),
                            &ceremony_id_typed,
                        )
                        .await
                    {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::KeyRotationCeremonyStatus {
                                ceremony_id: status.ceremony_id.to_string(),
                                kind: status.kind,
                                accepted_count: status.accepted_count,
                                total_count: status.total_count,
                                threshold: status.threshold,
                                is_complete: status.is_complete,
                                has_failed: status.has_failed,
                                accepted_participants: status.accepted_participants.clone(),
                                error_message: status.error_message.clone(),
                                pending_epoch: status.pending_epoch,
                                agreement_mode: status.agreement_mode,
                                reversion_risk: status.reversion_risk,
                            },
                        )
                        .await;
                    }

                    let app = ctx.app_core_raw().clone();
                    let tx_monitor = tx.clone();
                    let ceremony_id = CeremonyId::new(start.ceremony_id.clone());
                    spawn_ctx(ctx.clone(), async move {
                        let _ = aura_app::ui::workflows::ceremonies::monitor_key_rotation_ceremony(
                            &app,
                            ceremony_id,
                            Duration::from_millis(500),
                            |status| {
                                let _ = send_ui_update_lossy(
                                    &tx_monitor,
                                    UiUpdate::KeyRotationCeremonyStatus {
                                        ceremony_id: status.ceremony_id.to_string(),
                                        kind: status.kind,
                                        accepted_count: status.accepted_count,
                                        total_count: status.total_count,
                                        threshold: status.threshold,
                                        is_complete: status.is_complete,
                                        has_failed: status.has_failed,
                                        accepted_participants: status.accepted_participants.clone(),
                                        error_message: status.error_message.clone(),
                                        pending_epoch: status.pending_epoch,
                                        agreement_mode: status.agreement_mode,
                                        reversion_risk: status.reversion_risk,
                                    },
                                );
                            },
                            physical_sleep,
                        )
                        .await;
                    });
                });
            },
        )
    }

    fn make_remove_device(ctx: Arc<IoContext>, tx: UiUpdateSender) -> RemoveDeviceCallback {
        Arc::new(move |device_id| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let device_id_clone = device_id.to_string();

            spawn_ctx(ctx.clone(), async move {
                let ceremony_id = match ctx.start_device_removal(&device_id_clone).await {
                    Ok(id) => id,
                    Err(_e) => {
                        tracing::debug!(error = %_e, "dispatch error (surfaced via ERROR_SIGNAL)");
                        return;
                    }
                };

                send_ui_update_required(
                    &tx,
                    UiUpdate::ToastAdded(ToastMessage::info(
                        "device-removal-started",
                        "Device removal started",
                    )),
                )
                .await;

                #[cfg(feature = "development")]
                {
                    // In demo mode, make sure the simulated mobile device processes incoming
                    // threshold key packages so the removal ceremony can reach completion.
                    if device_id_clone == ctx.demo_mobile_device_id() {
                        let demo_ctx = ctx.clone();
                        spawn_ctx(ctx.clone(), async move {
                            for _ in 0..6 {
                                let _ = demo_ctx.process_demo_mobile_ceremony_acceptances().await;
                                physical_sleep(Duration::from_millis(150)).await;
                            }
                        });
                    }
                }

                // Best-effort: monitor completion and toast success/failure.
                let app = ctx.app_core_raw().clone();
                let tx_monitor = tx.clone();
                spawn_ctx(ctx.clone(), async move {
                    match aura_app::ui::workflows::ceremonies::monitor_key_rotation_ceremony(
                        &app,
                        CeremonyId::new(ceremony_id),
                        Duration::from_millis(250),
                        |_| {},
                        physical_sleep,
                    )
                    .await
                    {
                        Ok(status) if status.is_complete => {
                            send_ui_update_required(
                                &tx_monitor,
                                UiUpdate::ToastAdded(ToastMessage::success(
                                    "device-removal-complete",
                                    "Device removal complete",
                                )),
                            )
                            .await;
                        }
                        Ok(status) if status.has_failed => {
                            let msg = status
                                .error_message
                                .unwrap_or_else(|| "Device removal failed".to_string());
                            send_ui_update_required(
                                &tx_monitor,
                                UiUpdate::ToastAdded(ToastMessage::error(
                                    "device-removal-failed",
                                    msg,
                                )),
                            )
                            .await;
                        }
                        Ok(_) => {}
                        Err(_e) => {
                            // monitor already emitted error via ERROR_SIGNAL on polling failures.
                        }
                    }
                });
            });
        })
    }

    fn make_import_device_enrollment(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
    ) -> ImportDeviceEnrollmentCallback {
        Arc::new(move |code: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let should_complete_onboarding = !ctx.has_account();
            spawn_ctx(ctx.clone(), async move {
                match ctx.import_device_enrollment_code(&code).await {
                    Ok(()) => {
                        if should_complete_onboarding {
                            send_ui_update_required(&tx, UiUpdate::AccountCreated).await;
                        }
                        send_ui_update_required(
                            &tx,
                            UiUpdate::ToastAdded(ToastMessage::success(
                                "devices",
                                "Device enrollment invitation accepted",
                            )),
                        )
                        .await;
                    }
                    Err(e) => {
                        send_ui_update_required(
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

// =============================================================================
// Neighborhood Callbacks
// =============================================================================

/// All callbacks for the neighborhood screen
#[derive(Clone)]
pub struct NeighborhoodCallbacks {
    pub on_enter_home: Arc<dyn Fn(String, AccessLevel) + Send + Sync>,
    pub on_go_home: GoHomeCallback,
    pub on_back_to_limited: GoHomeCallback,
    pub on_set_moderator: SetModeratorCallback,
    pub on_create_home: CreateHomeCallback,
    pub on_create_neighborhood: CreateNeighborhoodCallback,
    pub on_add_home_to_neighborhood: NeighborhoodHomeCallback,
    pub on_link_home_one_hop_link: NeighborhoodHomeCallback,
}

impl NeighborhoodCallbacks {
    #[must_use]
    pub fn new(ctx: Arc<IoContext>, tx: UiUpdateSender) -> Self {
        Self {
            on_enter_home: Self::make_enter_home(ctx.clone(), tx.clone()),
            on_go_home: Self::make_go_home(ctx.clone(), tx.clone()),
            on_back_to_limited: Self::make_back_to_limited(ctx.clone(), tx.clone()),
            on_set_moderator: Self::make_set_moderator(ctx.clone(), tx.clone()),
            on_create_home: Self::make_create_home(ctx.clone(), tx.clone()),
            on_create_neighborhood: Self::make_create_neighborhood(ctx.clone(), tx.clone()),
            on_add_home_to_neighborhood: Self::make_add_home_to_neighborhood(
                ctx.clone(),
                tx.clone(),
            ),
            on_link_home_one_hop_link: Self::make_link_home_one_hop_link(ctx, tx),
        }
    }

    fn make_enter_home(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
    ) -> Arc<dyn Fn(String, AccessLevel) + Send + Sync> {
        Arc::new(move |home_id: String, depth: AccessLevel| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let home_id_clone = home_id.clone();
            let depth_str = match depth {
                AccessLevel::Limited => "Limited",
                AccessLevel::Partial => "Partial",
                AccessLevel::Full => "Full",
            }
            .to_string();
            let cmd = EffectCommand::MovePosition {
                neighborhood_id: "current".to_string(),
                home_id,
                depth: depth_str,
            };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::HomeEntered {
                                home_id: home_id_clone,
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

    fn make_go_home(ctx: Arc<IoContext>, tx: UiUpdateSender) -> GoHomeCallback {
        Arc::new(move || {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let cmd = EffectCommand::MovePosition {
                neighborhood_id: "current".to_string(),
                home_id: "home".to_string(),
                depth: "Full".to_string(),
            };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_required(&tx, UiUpdate::NavigatedHome).await;
                    }
                    Err(_e) => {
                        tracing::debug!(error = %_e, "dispatch error (surfaced via ERROR_SIGNAL)");
                    }
                }
            });
        })
    }

    fn make_back_to_limited(ctx: Arc<IoContext>, tx: UiUpdateSender) -> GoHomeCallback {
        Arc::new(move || {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let cmd = EffectCommand::MovePosition {
                neighborhood_id: "current".to_string(),
                home_id: "current".to_string(),
                depth: "Limited".to_string(),
            };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_required(&tx, UiUpdate::NavigatedToLimited).await;
                    }
                    Err(_e) => {
                        tracing::debug!(error = %_e, "dispatch error (surfaced via ERROR_SIGNAL)");
                    }
                }
            });
        })
    }

    fn make_set_moderator(ctx: Arc<IoContext>, tx: UiUpdateSender) -> SetModeratorCallback {
        Arc::new(
            move |home_id: Option<String>, target_id: String, assign: bool| {
                let ctx = ctx.clone();
                let tx = tx.clone();
                let cmd = if assign {
                    EffectCommand::GrantModerator {
                        channel: home_id,
                        target: target_id.clone(),
                    }
                } else {
                    EffectCommand::RevokeModerator {
                        channel: home_id,
                        target: target_id.clone(),
                    }
                };
                spawn_ctx(ctx.clone(), async move {
                    match ctx.dispatch(cmd).await {
                        Ok(_) => {
                            let action = if assign { "granted" } else { "revoked" };
                            send_ui_update_required(
                                &tx,
                                UiUpdate::ToastAdded(ToastMessage::success(
                                    "moderation",
                                    format!("Moderator designation {action} for {target_id}"),
                                )),
                            )
                            .await;
                        }
                        Err(e) => {
                            send_ui_update_required(
                                &tx,
                                UiUpdate::ToastAdded(ToastMessage::error(
                                    "moderation",
                                    format!("Failed to update moderator designation: {e}"),
                                )),
                            )
                            .await;
                        }
                    }
                });
            },
        )
    }

    fn make_create_home(ctx: Arc<IoContext>, tx: UiUpdateSender) -> CreateHomeCallback {
        Arc::new(move |name: String, _description: Option<String>| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let display_name = name.trim().to_string();
            let cmd = EffectCommand::CreateHome {
                name: Some(display_name.clone()),
            };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::ToastAdded(ToastMessage::success(
                                "home",
                                format!("Home '{display_name}' created"),
                            )),
                        )
                        .await;
                    }
                    Err(_error) => {}
                }
            });
        })
    }

    fn make_create_neighborhood(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
    ) -> CreateNeighborhoodCallback {
        Arc::new(move |name: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let display_name = if name.trim().is_empty() {
                "Neighborhood".to_string()
            } else {
                name.trim().to_string()
            };
            let cmd = EffectCommand::CreateNeighborhood {
                name: display_name.clone(),
            };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::ToastAdded(ToastMessage::success(
                                "neighborhood",
                                format!("Neighborhood '{display_name}' ready"),
                            )),
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

    fn make_add_home_to_neighborhood(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
    ) -> NeighborhoodHomeCallback {
        Arc::new(move |home_id: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let cmd = EffectCommand::AddHomeToNeighborhood { home_id };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::ToastAdded(ToastMessage::success(
                                "neighborhood",
                                "Home added to neighborhood",
                            )),
                        )
                        .await;
                    }
                    Err(e) => {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::ToastAdded(ToastMessage::error(
                                "neighborhood",
                                format!("Failed to add home to neighborhood: {e}"),
                            )),
                        )
                        .await;
                    }
                }
            });
        })
    }

    fn make_link_home_one_hop_link(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
    ) -> NeighborhoodHomeCallback {
        Arc::new(move |home_id: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let cmd = EffectCommand::LinkHomeOneHopLink { home_id };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::ToastAdded(ToastMessage::success(
                                "neighborhood",
                                "OneHopLink linked",
                            )),
                        )
                        .await;
                    }
                    Err(e) => {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::ToastAdded(ToastMessage::error(
                                "neighborhood",
                                format!("Failed to link one_hop_link: {e}"),
                            )),
                        )
                        .await;
                    }
                }
            });
        })
    }
}
