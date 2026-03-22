//! Settings and neighborhood domain callbacks.

use super::*;
use std::time::Duration;

use aura_core::effects::time::PhysicalTimeEffects;
use aura_effects::time::PhysicalTimeHandler;

use crate::tui::key_rotation::{key_rotation_lifecycle_toast, key_rotation_status_update};

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
    pub(crate) on_add_device: AddDeviceCallback,
    pub on_remove_device: RemoveDeviceCallback,
    pub(crate) on_import_device_enrollment_on_mobile: ImportDeviceEnrollmentCallback,
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
            spawn_observed_dispatch_callback(
                ctx.clone(),
                tx.clone(),
                EffectCommand::UpdateMfaPolicy {
                    require_mfa: policy.requires_mfa(),
                },
                move |tx| async move {
                    send_ui_update_required(&tx, UiUpdate::MfaPolicyChanged(policy)).await;
                },
                |error| async move {
                    tracing::debug!(error = %error, "dispatch error (surfaced via ERROR_SIGNAL)");
                },
            );
        })
    }

    fn make_update_nickname_suggestion(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
    ) -> UpdateNicknameSuggestionCallback {
        Arc::new(move |name: String| {
            let name_clone = name.clone();
            spawn_observed_dispatch_callback(
                ctx.clone(),
                tx.clone(),
                EffectCommand::UpdateNickname { name },
                move |tx| async move {
                    send_ui_update_required(&tx, UiUpdate::NicknameSuggestionChanged(name_clone))
                        .await;
                },
                |error| async move {
                    tracing::debug!(error = %error, "dispatch error (surfaced via ERROR_SIGNAL)");
                },
            );
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
                        ctx,
                        tx,
                        UiUpdate::operation_failed(
                            UiOperation::UpdateThreshold,
                            crate::error::TerminalError::Input(error),
                        ),
                    );
                    return;
                }
            };
            spawn_observed_dispatch_callback(
                ctx,
                tx,
                EffectCommand::UpdateThreshold { config },
                move |tx| async move {
                    send_ui_update_required(
                        &tx,
                        UiUpdate::ThresholdChanged {
                            k: threshold_k,
                            n: threshold_n,
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

    fn make_add_device(ctx: Arc<IoContext>, tx: UiUpdateSender) -> AddDeviceCallback {
        Arc::new(
            move |nickname_suggestion: String,
                  invitee_authority_id: AuthorityId,
                  operation: LocalTerminalOperationOwner| {
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
                            operation.fail(error.to_string()).await;
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
                    operation.succeed().await;

                    let status_handle = start.status_handle.clone();
                    ctx.remember_key_rotation_ceremony(start.cancel_handle)
                        .await;

                    // Prime status quickly (best-effort) so the modal has counters immediately.
                    if let Ok(status) =
                        aura_app::ui::workflows::ceremonies::get_key_rotation_ceremony_status(
                            ctx.app_core_raw(),
                            &status_handle,
                        )
                        .await
                    {
                        send_ui_update_required(&tx, key_rotation_status_update(&status)).await;
                    }

                    let tx_monitor = tx.clone();
                    spawn_ctx(ctx.clone(), async move {
                        let policy =
                            aura_app::ui::workflows::ceremonies::CeremonyPollPolicy::with_interval(
                                Duration::from_millis(500),
                            );
                        match aura_app::ui::workflows::ceremonies::monitor_key_rotation_ceremony_with_policy(
                            ctx.app_core_raw(),
                            &status_handle,
                            policy,
                            |status| {
                                let _ = send_ui_update_lossy(
                                    &tx_monitor,
                                    key_rotation_status_update(status),
                                );
                            },
                            physical_sleep,
                        )
                        .await
                        {
                            Ok(lifecycle) => {
                                if lifecycle.state
                                    == aura_app::ui::workflows::ceremonies::CeremonyLifecycleState::TimedOut
                                {
                                    let _ = send_ui_update_lossy(
                                        &tx_monitor,
                                        key_rotation_status_update(&lifecycle.status),
                                    );
                                }
                                if let Some(toast) =
                                    key_rotation_lifecycle_toast(lifecycle.status.kind, lifecycle.state)
                                {
                                    send_ui_update_required(&tx_monitor, UiUpdate::ToastAdded(toast))
                                        .await;
                                }
                            }
                            Err(error) => {
                                tracing::warn!(
                                    ceremony_id = %status_handle.ceremony_id(),
                                    error = %error,
                                    "device enrollment monitor failed"
                                );
                            }
                        }
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
                let handle = match ctx.start_device_removal(&device_id_clone).await {
                    Ok(handle) => handle,
                    Err(error) => {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::ToastAdded(ToastMessage::error(
                                "device-removal-failed",
                                format!("Device removal failed: {error}"),
                            )),
                        )
                        .await;
                        return;
                    }
                };
                let status_handle = handle.status_handle();

                send_ui_update_required(
                    &tx,
                    UiUpdate::ToastAdded(ToastMessage::info(
                        "device-removal-started",
                        "Device removal started",
                    )),
                )
                .await;

                ctx.remember_key_rotation_ceremony(handle).await;

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
                let tx_monitor = tx.clone();
                spawn_ctx(ctx.clone(), async move {
                    let policy = aura_app::ui::workflows::ceremonies::CeremonyPollPolicy {
                        interval: Duration::from_millis(250),
                        max_attempts: 160,
                        rollback_on_failure: true,
                        refresh_settings_on_complete: true,
                    };
                    match aura_app::ui::workflows::ceremonies::monitor_key_rotation_ceremony_with_policy(
                        ctx.app_core_raw(),
                        &status_handle,
                        policy,
                        |_| {},
                        physical_sleep,
                    )
                    .await
                    {
                        Ok(lifecycle) if lifecycle.status.is_complete => {
                            send_ui_update_required(
                                &tx_monitor,
                                UiUpdate::ToastAdded(ToastMessage::success(
                                    "device-removal-complete",
                                    "Device removal complete",
                                )),
                            )
                            .await;
                        }
                        Ok(lifecycle) if lifecycle.status.has_failed => {
                            let msg = lifecycle
                                .status
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
                        Ok(lifecycle) => {
                            send_ui_update_required(
                                &tx_monitor,
                                UiUpdate::ToastAdded(ToastMessage::error(
                                    "device-removal-timeout",
                                    format!(
                                        "Device removal did not settle before timeout ({:?})",
                                        lifecycle.state
                                    ),
                                )),
                            )
                            .await;
                        }
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
        Arc::new(
            move |code: String, operation: LocalTerminalOperationOwner| {
                let should_complete_onboarding = !ctx.has_account();
                spawn_local_terminal_result_callback(
                    ctx.clone(),
                    tx.clone(),
                    operation,
                    "ImportDeviceEnrollmentOnMobile callback",
                    move |ctx| async move { ctx.import_device_enrollment_code(&code).await },
                    move |tx, ()| async move {
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
                    },
                    |tx, error| async move {
                        send_ui_update_required(
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[test]
    fn add_device_monitor_uses_typed_lifecycle_outcomes() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let settings_path =
            repo_root.join("crates/aura-terminal/src/tui/callbacks/factories/settings.rs");
        let settings_source = std::fs::read_to_string(&settings_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", settings_path.display()));

        let add_device_start = settings_source
            .find("fn make_add_device(")
            .unwrap_or_else(|| panic!("missing make_add_device"));
        let remove_device_start = settings_source[add_device_start..]
            .find("fn make_remove_device(")
            .map(|offset| add_device_start + offset)
            .unwrap_or_else(|| panic!("missing make_remove_device"));
        let add_device_branch = &settings_source[add_device_start..remove_device_start];

        assert!(add_device_branch.contains("monitor_key_rotation_ceremony_with_policy("));
        assert!(!add_device_branch.contains("monitor_key_rotation_ceremony("));
        assert!(add_device_branch.contains("CeremonyLifecycleState::TimedOut"));
        assert!(add_device_branch.contains("key_rotation_lifecycle_toast("));
        assert!(settings_source.contains("use crate::tui::key_rotation::{"));
        assert!(!add_device_branch.contains("UiUpdate::KeyRotationCeremonyStatus {"));
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
            let home_id_clone = home_id.clone();
            let depth_str = match depth {
                AccessLevel::Limited => "Limited",
                AccessLevel::Partial => "Partial",
                AccessLevel::Full => "Full",
            }
            .to_string();
            spawn_observed_dispatch_callback(
                ctx.clone(),
                tx.clone(),
                EffectCommand::MovePosition {
                    neighborhood_id: "current".to_string(),
                    home_id,
                    depth: depth_str,
                },
                move |tx| async move {
                    send_ui_update_required(
                        &tx,
                        UiUpdate::HomeEntered {
                            home_id: home_id_clone,
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

    fn make_go_home(ctx: Arc<IoContext>, tx: UiUpdateSender) -> GoHomeCallback {
        Arc::new(move || {
            spawn_observed_dispatch_callback(
                ctx.clone(),
                tx.clone(),
                EffectCommand::MovePosition {
                    neighborhood_id: "current".to_string(),
                    home_id: "home".to_string(),
                    depth: "Full".to_string(),
                },
                |tx| async move {
                    send_ui_update_required(&tx, UiUpdate::NavigatedHome).await;
                },
                |error| async move {
                    tracing::debug!(error = %error, "dispatch error (surfaced via ERROR_SIGNAL)");
                },
            );
        })
    }

    fn make_back_to_limited(ctx: Arc<IoContext>, tx: UiUpdateSender) -> GoHomeCallback {
        Arc::new(move || {
            spawn_observed_dispatch_callback(
                ctx.clone(),
                tx.clone(),
                EffectCommand::MovePosition {
                    neighborhood_id: "current".to_string(),
                    home_id: "current".to_string(),
                    depth: "Limited".to_string(),
                },
                |tx| async move {
                    send_ui_update_required(&tx, UiUpdate::NavigatedToLimited).await;
                },
                |error| async move {
                    tracing::debug!(error = %error, "dispatch error (surfaced via ERROR_SIGNAL)");
                },
            );
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
                spawn_observed_dispatch_callback(
                    ctx.clone(),
                    tx.clone(),
                    cmd,
                    move |tx| async move {
                        let action = if assign { "granted" } else { "revoked" };
                        send_ui_update_required(
                            &tx,
                            UiUpdate::ToastAdded(ToastMessage::success(
                                "moderation",
                                format!("Moderator designation {action} for {target_id}"),
                            )),
                        )
                        .await;
                    },
                    |error| async move {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::ToastAdded(ToastMessage::error(
                                "moderation",
                                format!("Failed to update moderator designation: {error}"),
                            )),
                        )
                        .await;
                    },
                );
            },
        )
    }

    fn make_create_home(ctx: Arc<IoContext>, tx: UiUpdateSender) -> CreateHomeCallback {
        Arc::new(move |name: String, _description: Option<String>| {
            let display_name = name.trim().to_string();
            let failure_tx = tx.clone();
            spawn_observed_dispatch_callback(
                ctx.clone(),
                tx.clone(),
                EffectCommand::CreateHome {
                    name: Some(display_name.clone()),
                },
                move |tx| async move {
                    send_ui_update_required(
                        &tx,
                        UiUpdate::ToastAdded(ToastMessage::success(
                            "home",
                            format!("Home '{display_name}' created"),
                        )),
                    )
                    .await;
                },
                move |error| async move {
                    send_ui_update_required(
                        &failure_tx,
                        UiUpdate::ToastAdded(ToastMessage::error(
                            "home",
                            format!("Failed to create home: {error}"),
                        )),
                    )
                    .await;
                },
            );
        })
    }

    fn make_create_neighborhood(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
    ) -> CreateNeighborhoodCallback {
        Arc::new(move |name: String| {
            let display_name = if name.trim().is_empty() {
                "Neighborhood".to_string()
            } else {
                name.trim().to_string()
            };
            let failure_tx = tx.clone();
            spawn_observed_dispatch_callback(
                ctx.clone(),
                tx.clone(),
                EffectCommand::CreateNeighborhood {
                    name: display_name.clone(),
                },
                move |tx| async move {
                    send_ui_update_required(
                        &tx,
                        UiUpdate::ToastAdded(ToastMessage::success(
                            "neighborhood",
                            format!("Neighborhood '{display_name}' ready"),
                        )),
                    )
                    .await;
                },
                move |error| async move {
                    send_ui_update_required(
                        &failure_tx,
                        UiUpdate::ToastAdded(ToastMessage::error(
                            "neighborhood",
                            format!("Failed to create neighborhood: {error}"),
                        )),
                    )
                    .await;
                },
            );
        })
    }

    fn make_add_home_to_neighborhood(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
    ) -> NeighborhoodHomeCallback {
        Arc::new(move |home_id: String| {
            let failure_tx = tx.clone();
            spawn_observed_dispatch_callback(
                ctx.clone(),
                tx.clone(),
                EffectCommand::AddHomeToNeighborhood { home_id },
                |tx| async move {
                    send_ui_update_required(
                        &tx,
                        UiUpdate::ToastAdded(ToastMessage::success(
                            "neighborhood",
                            "Home added to neighborhood",
                        )),
                    )
                    .await;
                },
                move |error| async move {
                    send_ui_update_required(
                        &failure_tx,
                        UiUpdate::ToastAdded(ToastMessage::error(
                            "neighborhood",
                            format!("Failed to add home to neighborhood: {error}"),
                        )),
                    )
                    .await;
                },
            );
        })
    }

    fn make_link_home_one_hop_link(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
    ) -> NeighborhoodHomeCallback {
        Arc::new(move |home_id: String| {
            let failure_tx = tx.clone();
            spawn_observed_dispatch_callback(
                ctx.clone(),
                tx.clone(),
                EffectCommand::LinkHomeOneHopLink { home_id },
                |tx| async move {
                    send_ui_update_required(
                        &tx,
                        UiUpdate::ToastAdded(ToastMessage::success(
                            "neighborhood",
                            "OneHopLink linked",
                        )),
                    )
                    .await;
                },
                move |error| async move {
                    send_ui_update_required(
                        &failure_tx,
                        UiUpdate::ToastAdded(ToastMessage::error(
                            "neighborhood",
                            format!("Failed to link one_hop_link: {error}"),
                        )),
                    )
                    .await;
                },
            );
        })
    }
}
