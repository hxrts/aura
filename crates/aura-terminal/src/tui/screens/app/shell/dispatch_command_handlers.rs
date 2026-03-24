use super::dispatch::*;
use super::*;

use aura_app::ui::workflows::access as access_workflows;
use aura_app::ui::workflows::ceremonies::{
    monitor_key_rotation_ceremony_with_policy, start_device_threshold_ceremony,
    start_guardian_ceremony, CeremonyLifecycleState, CeremonyPollPolicy,
};
use aura_app::ui_contract::SemanticOperationKind;
use aura_core::types::FrostThreshold;

use crate::tui::key_rotation::{key_rotation_lifecycle_toast, key_rotation_status_update};
use crate::tui::semantic_lifecycle::{LocalTerminalOperationOwner, WorkflowHandoffOperationOwner};
use crate::tui::updates::UiOperation;

fn handle_recovery_and_ceremonies_dispatch(
    dispatch_cmd: DispatchCommand,
    new_state: &mut TuiState,
    event_ctx: &EventDispatchContext<'_>,
) -> EventCommandLoopAction {
    let cb = event_ctx.callbacks;
    let app_core_for_events = event_ctx.app_ctx.app_core.raw().clone();
    let app_core_for_ceremony = event_ctx.app_ctx.app_core.clone();
    let io_ctx_for_ceremony = event_ctx.app_ctx.clone();
    let update_tx_for_events = event_ctx.update_tx_for_events.clone();
    let update_tx_for_ceremony = event_ctx.update_tx_for_ceremony.clone();
    let tasks_for_events = event_ctx.tasks_for_events.clone();
    let shared_invitations_for_dispatch = event_ctx.shared_invitations_for_dispatch;
    let shared_pending_requests_for_dispatch = event_ctx.shared_pending_requests_for_dispatch;
    let shared_contacts_for_dispatch = event_ctx.shared_contacts_for_dispatch;
    let shared_threshold_for_dispatch = event_ctx.shared_threshold_for_dispatch;

    match dispatch_cmd {
        DispatchCommand::StartRecovery => {
            let (threshold_k, _threshold_n) = *shared_threshold_for_dispatch.read();
            if threshold_k == 0 {
                new_state.toast_error(RecoveryError::NoThresholdConfigured.to_string());
                return EventCommandLoopAction::ContinueCommand;
            }

            let guardian_count = shared_contacts_for_dispatch
                .read()
                .iter()
                .filter(|contact| contact.is_guardian)
                .count();

            if guardian_count < threshold_k as usize {
                new_state.toast_error(
                    RecoveryError::InsufficientGuardians {
                        required: threshold_k,
                        available: guardian_count,
                    }
                    .to_string(),
                );
                return EventCommandLoopAction::ContinueCommand;
            }

            let Some(update_tx) = update_tx_for_events.clone() else {
                new_state.toast_error("UI update sender is unavailable");
                return EventCommandLoopAction::ContinueCommand;
            };
            let operation = LocalTerminalOperationOwner::submit(
                app_core_for_events.clone(),
                tasks_for_events.clone(),
                update_tx,
                OperationId::start_recovery(),
                SemanticOperationKind::StartRecovery,
            );
            (cb.recovery.on_start_recovery)(operation);
        }
        DispatchCommand::ApproveRecovery => {
            let selected = read_selected_notification(
                new_state.notifications.selected_index,
                &shared_invitations_for_dispatch,
                &shared_pending_requests_for_dispatch,
            );
            let approval_target = match selected {
                Some(NotificationSelection::RecoveryRequest(request_id)) => Some(request_id),
                _ => {
                    let guard = shared_pending_requests_for_dispatch.read();
                    guard.first().map(|request| request.id.clone())
                }
            };
            if let Some(request_id) = approval_target {
                let Some(update_tx) = update_tx_for_events.clone() else {
                    new_state.toast_error("UI update sender is unavailable");
                    return EventCommandLoopAction::ContinueCommand;
                };
                let operation = LocalTerminalOperationOwner::submit(
                    app_core_for_events.clone(),
                    tasks_for_events.clone(),
                    update_tx,
                    OperationId::submit_guardian_approval(),
                    SemanticOperationKind::SubmitGuardianApproval,
                );
                (cb.recovery.on_submit_approval)(request_id, operation);
            } else {
                new_state.toast_error("No pending recovery requests");
            }
        }
        DispatchCommand::StartGuardianCeremony {
            contact_ids,
            threshold_k,
        } => {
            tracing::info!(
                "Starting guardian ceremony with {} contacts, threshold {}",
                contact_ids.len(),
                threshold_k.get()
            );

            let ids = contact_ids.clone();
            let n = contact_ids.len() as u16;
            let k_raw = threshold_k.get() as u16;
            let threshold = match FrostThreshold::new(k_raw) {
                Ok(threshold) => threshold,
                Err(error) => {
                    tracing::error!("Invalid threshold for guardian ceremony: {}", error);
                    let update_tx = update_tx_for_ceremony.clone();
                    let tasks = tasks_for_events.clone();
                    tasks.spawn(async move {
                        send_optional_ui_update_required(
                            &update_tx,
                            UiUpdate::operation_failed(
                                UiOperation::StartGuardianCeremony,
                                TerminalError::Input(error.to_string()),
                            ),
                        )
                        .await;
                    });
                    return EventCommandLoopAction::ContinueCommand;
                }
            };

            let app_core = app_core_for_ceremony.clone();
            let io_ctx = io_ctx_for_ceremony.clone();
            let update_tx = update_tx_for_ceremony.clone();

            let tasks = tasks_for_events.clone();
            let tasks_handle = tasks.clone();
            tasks_handle.spawn(async move {
                let app = app_core.raw();
                match start_guardian_ceremony(app, threshold, n, ids).await {
                    Ok(ceremony_handle) => {
                        let status_handle = ceremony_handle.status_handle();
                        io_ctx.remember_key_rotation_ceremony(ceremony_handle).await;
                        let k = threshold.value();
                        tracing::info!(
                            ceremony_id = %status_handle.ceremony_id(),
                            threshold = k,
                            guardians = n,
                            "Guardian ceremony initiated, waiting for guardian responses"
                        );

                        if let Some(tx) = update_tx.clone() {
                            let _ = tx.try_send(UiUpdate::ToastAdded(ToastMessage::info(
                                "guardian-ceremony-started",
                                format!(
                                    "Guardian ceremony started! Waiting for {k}-of-{n} guardians to respond"
                                ),
                            )));

                            let _ = tx.try_send(UiUpdate::KeyRotationCeremonyStatus {
                                ceremony_id: status_handle.ceremony_id().to_string(),
                                kind: aura_app::ui::types::CeremonyKind::GuardianRotation,
                                accepted_count: 0,
                                total_count: n,
                                threshold: k,
                                is_complete: false,
                                has_failed: false,
                                accepted_participants: Vec::new(),
                                error_message: None,
                                pending_epoch: None,
                                agreement_mode: aura_core::threshold::policy_for(
                                    aura_core::threshold::CeremonyFlow::GuardianSetupRotation,
                                )
                                .initial_mode(),
                                reversion_risk: true,
                            });
                        }

                        let app_core_monitor = app.clone();
                        let update_tx_monitor = update_tx.clone();
                        let tasks = tasks.clone();
                        let tasks_handle = tasks;
                        tasks_handle.spawn(async move {
                            let policy = CeremonyPollPolicy::with_interval(
                                std::time::Duration::from_millis(500),
                            );
                            match monitor_key_rotation_ceremony_with_policy(
                                &app_core_monitor,
                                &status_handle,
                                policy,
                                |status| {
                                    if let Some(tx) = update_tx_monitor.clone() {
                                        let _ = tx.try_send(key_rotation_status_update(status));
                                    }
                                },
                                effect_sleep,
                            )
                            .await
                            {
                                Ok(lifecycle) => {
                                    if lifecycle.state == CeremonyLifecycleState::TimedOut {
                                        if let Some(tx) = update_tx_monitor.clone() {
                                            let _ = tx.try_send(key_rotation_status_update(
                                                &lifecycle.status,
                                            ));
                                        }
                                    }
                                    if let Some(toast) = key_rotation_lifecycle_toast(
                                        lifecycle.status.kind,
                                        lifecycle.state,
                                    ) {
                                        if let Some(tx) = update_tx_monitor.clone() {
                                            let _ = tx.try_send(UiUpdate::ToastAdded(toast));
                                        }
                                    }
                                }
                                Err(error) => {
                                    tracing::warn!(
                                        ceremony_id = %status_handle.ceremony_id(),
                                        error = %error,
                                        "guardian ceremony monitor failed"
                                    );
                                }
                            }
                        });
                    }
                    Err(error) => {
                        tracing::error!("Failed to initiate guardian ceremony: {}", error);

                        if let Some(tx) = update_tx {
                            send_optional_ui_update_required(
                                &Some(tx),
                                UiUpdate::operation_failed(
                                    UiOperation::StartGuardianCeremony,
                                    TerminalError::Operation(error.to_string()),
                                ),
                            )
                            .await;
                        }
                    }
                }
            });
        }
        DispatchCommand::StartMfaCeremony {
            device_ids,
            threshold_k,
        } => {
            tracing::info!(
                "Starting multifactor ceremony with {} devices, threshold {}",
                device_ids.len(),
                threshold_k.get()
            );

            let ids = device_ids.clone();
            let n = device_ids.len() as u16;
            let k_raw = threshold_k.get() as u16;
            let threshold = match FrostThreshold::new(k_raw) {
                Ok(threshold) => threshold,
                Err(error) => {
                    tracing::error!("Invalid threshold for multifactor ceremony: {}", error);
                    let update_tx = update_tx_for_ceremony.clone();
                    let tasks = tasks_for_events.clone();
                    tasks.spawn(async move {
                        send_optional_ui_update_required(
                            &update_tx,
                            UiUpdate::operation_failed(
                                UiOperation::StartMultifactorCeremony,
                                TerminalError::Input(error.to_string()),
                            ),
                        )
                        .await;
                    });
                    return EventCommandLoopAction::ContinueCommand;
                }
            };

            let app_core = app_core_for_ceremony.clone();
            let io_ctx = io_ctx_for_ceremony.clone();
            let update_tx = update_tx_for_ceremony.clone();

            let tasks = tasks_for_events.clone();
            let tasks_handle = tasks.clone();
            tasks_handle.spawn(async move {
                let app = app_core.raw();

                match start_device_threshold_ceremony(
                    app,
                    threshold,
                    n,
                    ids.iter().map(|id| id.to_string()).collect(),
                )
                .await
                {
                    Ok(ceremony_handle) => {
                        let status_handle = ceremony_handle.status_handle();
                        io_ctx.remember_key_rotation_ceremony(ceremony_handle).await;
                        let k = threshold.value();
                        tracing::info!(
                            "Multifactor ceremony initiated: {} ({}-of-{})",
                            status_handle.ceremony_id(),
                            k,
                            n
                        );

                        if let Some(tx) = update_tx.clone() {
                            let _ = tx.try_send(UiUpdate::ToastAdded(ToastMessage::info(
                                "mfa-ceremony-started",
                                format!("Multifactor ceremony started ({k}-of-{n})"),
                            )));
                        }

                        if let Some(tx) = update_tx.clone() {
                            let _ = tx.try_send(UiUpdate::KeyRotationCeremonyStatus {
                                ceremony_id: status_handle.ceremony_id().to_string(),
                                kind: aura_app::ui::types::CeremonyKind::DeviceRotation,
                                accepted_count: 0,
                                total_count: n,
                                threshold: k,
                                is_complete: false,
                                has_failed: false,
                                accepted_participants: Vec::new(),
                                error_message: None,
                                pending_epoch: None,
                                agreement_mode: aura_core::threshold::policy_for(
                                    aura_core::threshold::CeremonyFlow::DeviceMfaRotation,
                                )
                                .initial_mode(),
                                reversion_risk: true,
                            });
                        }

                        let app_core_monitor = app.clone();
                        let update_tx_monitor = update_tx.clone();
                        let tasks = tasks.clone();
                        let tasks_handle = tasks;
                        tasks_handle.spawn(async move {
                            let policy = CeremonyPollPolicy::with_interval(
                                std::time::Duration::from_millis(500),
                            );
                            match monitor_key_rotation_ceremony_with_policy(
                                &app_core_monitor,
                                &status_handle,
                                policy,
                                |status| {
                                    if let Some(tx) = update_tx_monitor.clone() {
                                        let _ = tx.try_send(key_rotation_status_update(status));
                                    }
                                },
                                effect_sleep,
                            )
                            .await
                            {
                                Ok(lifecycle) => {
                                    if lifecycle.state == CeremonyLifecycleState::TimedOut {
                                        if let Some(tx) = update_tx_monitor.clone() {
                                            let _ = tx.try_send(key_rotation_status_update(
                                                &lifecycle.status,
                                            ));
                                        }
                                    }
                                    if let Some(toast) = key_rotation_lifecycle_toast(
                                        lifecycle.status.kind,
                                        lifecycle.state,
                                    ) {
                                        if let Some(tx) = update_tx_monitor.clone() {
                                            let _ = tx.try_send(UiUpdate::ToastAdded(toast));
                                        }
                                    }
                                }
                                Err(error) => {
                                    tracing::warn!(
                                        ceremony_id = %status_handle.ceremony_id(),
                                        error = %error,
                                        "multifactor ceremony monitor failed"
                                    );
                                }
                            }
                        });
                    }
                    Err(error) => {
                        tracing::error!("Failed to initiate multifactor ceremony: {}", error);

                        if let Some(tx) = update_tx {
                            send_optional_ui_update_required(
                                &Some(tx),
                                UiUpdate::operation_failed(
                                    UiOperation::StartMultifactorCeremony,
                                    TerminalError::Operation(error.to_string()),
                                ),
                            )
                            .await;
                        }
                    }
                }
            });
        }
        DispatchCommand::CancelGuardianCeremony { ceremony_id } => {
            tracing::info!(ceremony_id = %ceremony_id, "Canceling guardian ceremony");

            let app_core = app_core_for_ceremony.clone();
            let io_ctx = io_ctx_for_ceremony.clone();
            let update_tx = update_tx_for_ceremony.clone();

            let tasks = tasks_for_events.clone();
            tasks.spawn(async move {
                let app = app_core.raw();
                let handle = match io_ctx.take_key_rotation_ceremony_handle(&ceremony_id).await {
                    Ok(handle) => handle,
                    Err(error) => {
                        tracing::error!("Failed to resolve guardian ceremony handle: {}", error);
                        send_optional_ui_update_required(
                            &update_tx,
                            UiUpdate::operation_failed(
                                UiOperation::CancelGuardianCeremony,
                                TerminalError::Operation(error.to_string()),
                            ),
                        )
                        .await;
                        return;
                    }
                };
                if let Err(error) =
                    aura_app::ui::workflows::ceremonies::cancel_key_rotation_ceremony(app, handle)
                        .await
                {
                    tracing::error!("Failed to cancel guardian ceremony: {}", error);
                    send_optional_ui_update_required(
                        &update_tx,
                        UiUpdate::operation_failed(
                            UiOperation::CancelGuardianCeremony,
                            TerminalError::Operation(error.to_string()),
                        ),
                    )
                    .await;
                    return;
                }
                io_ctx.forget_key_rotation_ceremony(&ceremony_id).await;

                if let Some(tx) = update_tx {
                    let _ = tx.try_send(UiUpdate::ToastAdded(ToastMessage::info(
                        "guardian-ceremony-canceled",
                        "Guardian ceremony canceled",
                    )));
                }
            });
        }
        DispatchCommand::CancelKeyRotationCeremony { ceremony_id } => {
            tracing::info!(ceremony_id = %ceremony_id, "Canceling ceremony");

            let app_core = app_core_for_ceremony.clone();
            let io_ctx = io_ctx_for_ceremony.clone();
            let update_tx = update_tx_for_ceremony.clone();

            let tasks = tasks_for_events.clone();
            tasks.spawn(async move {
                let app = app_core.raw();
                let handle = match io_ctx.take_key_rotation_ceremony_handle(&ceremony_id).await {
                    Ok(handle) => handle,
                    Err(error) => {
                        tracing::error!("Failed to resolve ceremony handle: {}", error);
                        send_optional_ui_update_required(
                            &update_tx,
                            UiUpdate::operation_failed(
                                UiOperation::CancelKeyRotationCeremony,
                                TerminalError::Operation(error.to_string()),
                            ),
                        )
                        .await;
                        return;
                    }
                };
                if let Err(error) =
                    aura_app::ui::workflows::ceremonies::cancel_key_rotation_ceremony(app, handle)
                        .await
                {
                    tracing::error!("Failed to cancel ceremony: {}", error);
                    send_optional_ui_update_required(
                        &update_tx,
                        UiUpdate::operation_failed(
                            UiOperation::CancelKeyRotationCeremony,
                            TerminalError::Operation(error.to_string()),
                        ),
                    )
                    .await;
                    return;
                }
                io_ctx.forget_key_rotation_ceremony(&ceremony_id).await;

                if let Some(tx) = update_tx {
                    let _ = tx.try_send(UiUpdate::ToastAdded(ToastMessage::info(
                        "ceremony-canceled",
                        "Ceremony canceled",
                    )));
                }
            });
        }
        _ => unreachable!("unexpected dispatch command routed to ceremonies module"),
    }

    EventCommandLoopAction::Handled
}

pub(super) fn handle_dispatch_command_match(
    dispatch_cmd: DispatchCommand,
    new_state: &mut TuiState,
    event_ctx: &EventDispatchContext<'_>,
) -> EventCommandLoopAction {
    let cb = event_ctx.callbacks;
    let app_ctx_for_dispatch = event_ctx.app_ctx.clone();
    let app_core_for_events = event_ctx.app_ctx.app_core.raw().clone();
    let app_core_for_ceremony = event_ctx.app_ctx.app_core.clone();
    let update_tx_for_events = event_ctx.update_tx_for_events.clone();
    let update_tx_for_dispatch = event_ctx.update_tx_for_dispatch.clone();
    let update_tx_for_ceremony = event_ctx.update_tx_for_ceremony.clone();
    let tasks_for_events = event_ctx.tasks_for_events.clone();
    let shared_channels_for_dispatch = event_ctx.shared_channels_for_dispatch;
    let shared_neighborhood_homes_for_dispatch = event_ctx.shared_neighborhood_homes_for_dispatch;
    let shared_invitations_for_dispatch = event_ctx.shared_invitations_for_dispatch;
    let shared_pending_requests_for_dispatch = event_ctx.shared_pending_requests_for_dispatch;
    let shared_contacts_for_dispatch = event_ctx.shared_contacts_for_dispatch;
    let shared_discovered_peers_for_dispatch = event_ctx.shared_discovered_peers_for_dispatch;
    let shared_messages_for_dispatch = event_ctx.shared_messages_for_dispatch;
    let tui_selected_for_events = event_ctx.tui_selected_for_events;
    let selected_channel_binding_for_events = event_ctx.selected_channel_binding_for_events;

    match dispatch_cmd {
        DispatchCommand::CreateAccount { name } => {
            let Some(update_tx) = update_tx_for_events.clone() else {
                new_state.toast_error("UI update sender is unavailable");
                return EventCommandLoopAction::ContinueCommand;
            };
            let operation = LocalTerminalOperationOwner::submit(
                app_core_for_events.clone(),
                tasks_for_events.clone(),
                update_tx,
                OperationId::account_create(),
                SemanticOperationKind::CreateAccount,
            );
            let handle = operation.harness_handle();
            new_state.set_authoritative_operation_state(
                handle.operation_id().clone(),
                Some(handle.instance_id().clone()),
                None,
                OperationState::Submitting,
            );
            (cb.app.on_create_account)(name, operation);
        }
        DispatchCommand::ImportDeviceEnrollmentDuringOnboarding { code } => {
            let Some(update_tx) = update_tx_for_events.clone() else {
                new_state.toast_error("UI update sender is unavailable");
                return EventCommandLoopAction::ContinueCommand;
            };
            let operation = LocalTerminalOperationOwner::submit(
                app_core_for_events.clone(),
                tasks_for_events.clone(),
                update_tx,
                OperationId::device_enrollment(),
                SemanticOperationKind::ImportDeviceEnrollmentCode,
            );
            (cb.app.on_import_device_enrollment_during_onboarding)(code, operation);
        }
        DispatchCommand::AddGuardian { contact_id } => {
            (cb.recovery.on_select_guardian)(contact_id.to_string());
        }

        // === Chat Screen Commands ===
        DispatchCommand::SelectChannel { channel_id } => {
            let channels = shared_channels_for_dispatch.read().clone();
            if let Some(idx) = channels.iter().position(|channel| channel.id == channel_id) {
                new_state.chat.selected_channel = idx;
                *tui_selected_for_events.write() =
                    Some(CommittedChannelSelection::new(channel_id.to_string()));
                {
                    let mut guard = selected_channel_binding_for_events.write();
                    *guard = channels.get(idx).map(authoritative_channel_binding);
                }
            }
        }
        DispatchCommand::JoinChannel { channel_name } => {
            let Some(update_tx) = update_tx_for_events.clone() else {
                new_state.toast_error("UI update sender is unavailable");
                return EventCommandLoopAction::ContinueCommand;
            };
            let operation = WorkflowHandoffOperationOwner::submit(
                app_core_for_events.clone(),
                tasks_for_events.clone(),
                update_tx,
                OperationId::join_channel(),
                SemanticOperationKind::JoinChannel,
            );
            new_state.router.go_to(Screen::Chat);
            (cb.chat.on_join_channel)(channel_name, operation);
        }
        DispatchCommand::AcceptPendingHomeInvitation => {
            let Some(update_tx) = update_tx_for_events.clone() else {
                new_state.toast_error("UI update sender is unavailable");
                return EventCommandLoopAction::ContinueCommand;
            };
            let operation = WorkflowHandoffOperationOwner::submit(
                app_core_for_events.clone(),
                tasks_for_events.clone(),
                update_tx,
                OperationId::invitation_accept(),
                SemanticOperationKind::AcceptPendingChannelInvitation,
            );
            new_state.router.go_to(Screen::Chat);
            (cb.chat.on_accept_pending_channel_invitation)(operation);
        }
        DispatchCommand::SendChatMessage { content } => {
            let trimmed = content.trim_start();
            let operation = if trimmed.starts_with('/') {
                None
            } else {
                let Some(update_tx) = update_tx_for_events.clone() else {
                    new_state.toast_error("UI update sender is unavailable");
                    return EventCommandLoopAction::ContinueCommand;
                };
                Some(WorkflowHandoffOperationOwner::submit(
                    app_core_for_events.clone(),
                    tasks_for_events.clone(),
                    update_tx,
                    OperationId::send_message(),
                    SemanticOperationKind::SendChatMessage,
                ))
            };
            let channels = shared_channels_for_dispatch.read().clone();
            let committed_channel_id = tui_selected_for_events
                .read()
                .clone()
                .filter(|selection| {
                    channels.is_empty()
                        || channels
                            .iter()
                            .any(|channel| channel.id == selection.channel_id())
                })
                .map(|selection| selection.channel_id().to_string());
            if let Some(channel_id) = committed_channel_id.or_else(|| {
                resolve_committed_selected_channel_id(&new_state, &channels)
                    .map(|selection| selection.channel_id().to_string())
            }) {
                if let Some(operation) = operation {
                    (cb.chat.on_send_owned)(channel_id, content, operation);
                } else {
                    (cb.chat.on_send)(channel_id, content);
                }
            } else {
                new_state.toast_error(format!(
                    "No committed channel selected (channels={} selected_index={})",
                    channels.len(),
                    new_state.chat.selected_channel,
                ));
            }
        }
        DispatchCommand::RetryMessage => {
            let idx = new_state.chat.message_scroll;
            let guard = shared_messages_for_dispatch.read();
            if let Some(msg) = guard.get(idx) {
                (cb.chat.on_retry_message)(
                    msg.id.clone(),
                    msg.channel_id.clone(),
                    msg.content.clone(),
                );
            } else {
                new_state.toast_error("No message selected");
            }
        }
        DispatchCommand::OpenChatCreateWizard => {
            let current_contacts = shared_contacts_for_dispatch.read().clone();

            // Validate: need at least 1 contact (+ self = 2 participants)
            if current_contacts.is_empty() {
                new_state.toast_error(
                    ChannelError::InsufficientParticipants {
                        required: MIN_CHANNEL_PARTICIPANTS,
                        available: 1, // Just self
                    }
                    .to_string(),
                );
                return EventCommandLoopAction::ContinueCommand;
            }

            let mut candidates: Vec<crate::tui::state::ChatMemberCandidate> = current_contacts
                .iter()
                // Channel member invites only support user authorities.
                .filter(|c| c.id.starts_with("authority-"))
                .map(|c| crate::tui::state::ChatMemberCandidate {
                    id: c.id.clone(),
                    name: if !c.nickname.is_empty() {
                        c.nickname.clone()
                    } else if let Some(s) = &c.nickname_suggestion {
                        s.clone()
                    } else {
                        let short = c.id.chars().take(8).collect::<String>();
                        format!("{short}...")
                    },
                })
                .collect();
            let demo_alice_id = crate::ids::authority_id(&format!(
                "demo:{}:{}:authority",
                aura_app::ui::workflows::demo_config::DEMO_SEED_2024,
                "Alice"
            ))
            .to_string();
            let demo_carol_id = crate::ids::authority_id(&format!(
                "demo:{}:{}:authority",
                aura_app::ui::workflows::demo_config::DEMO_SEED_2024 + 1,
                "Carol"
            ))
            .to_string();
            let is_demo_mode = candidates
                .iter()
                .any(|candidate| candidate.id == demo_alice_id || candidate.id == demo_carol_id);
            let demo_name_rank = |contact_id: &str, name: &str| -> u8 {
                if !is_demo_mode {
                    return 2;
                }
                if name.eq_ignore_ascii_case("Alice") || demo_alice_id == contact_id {
                    0
                } else if name.eq_ignore_ascii_case("Carol") || demo_carol_id == contact_id {
                    1
                } else {
                    2
                }
            };
            candidates.sort_by(|left, right| {
                demo_name_rank(&left.id, &left.name)
                    .cmp(&demo_name_rank(&right.id, &right.name))
                    .then_with(|| {
                        left.name
                            .to_ascii_lowercase()
                            .cmp(&right.name.to_ascii_lowercase())
                    })
            });

            let mut modal_state = crate::tui::state::CreateChannelModalState::new();
            modal_state.contacts = candidates;
            modal_state.ensure_threshold();

            new_state
                .modal_queue
                .enqueue(crate::tui::state::QueuedModal::ChatCreate(modal_state));
        }

        DispatchCommand::CreateChannel {
            name,
            topic,
            mut members,
            threshold_k,
        } => {
            // Demo safeguard: keep the canonical trio room aligned with
            // Alice+Carol participation even if picker timing drifts.
            if name.eq_ignore_ascii_case("demo-trio-room") {
                let contacts = shared_contacts_for_dispatch.read().clone();
                let mut demo_members = Vec::new();
                let expected_demo_ids = [
                    crate::ids::authority_id(&format!(
                        "demo:{}:{}:authority",
                        aura_app::ui::workflows::demo_config::DEMO_SEED_2024,
                        "Alice"
                    ))
                    .to_string(),
                    crate::ids::authority_id(&format!(
                        "demo:{}:{}:authority",
                        aura_app::ui::workflows::demo_config::DEMO_SEED_2024 + 1,
                        "Carol"
                    ))
                    .to_string(),
                ];
                for expected_id in expected_demo_ids {
                    if contacts.iter().any(|contact| contact.id == expected_id) {
                        if let Ok(parsed_id) = expected_id.parse::<aura_core::AuthorityId>() {
                            demo_members.push(parsed_id);
                        }
                    }
                }
                // Fallback if deterministic IDs are unavailable in the contact list.
                for needle in ["Alice", "Carol"] {
                    if demo_members.len() >= 2 {
                        break;
                    }
                    if let Some(contact_id) = contacts.iter().find_map(|contact| {
                        let nickname = contact.nickname.trim();
                        let suggested = contact.nickname_suggestion.as_deref().unwrap_or("").trim();
                        if nickname.eq_ignore_ascii_case(needle)
                            || suggested.eq_ignore_ascii_case(needle)
                        {
                            Some(contact.id.clone())
                        } else {
                            None
                        }
                    }) {
                        if let Ok(parsed_id) = contact_id.parse::<aura_core::AuthorityId>() {
                            demo_members.push(parsed_id);
                        }
                    }
                }
                if !demo_members.is_empty() {
                    tracing::debug!(
                        room = %name,
                        ?members,
                        ?demo_members,
                        "Applying demo trio membership override"
                    );
                    members = demo_members;
                }
            }
            members.sort();
            members.dedup();
            let Some(update_tx) = update_tx_for_events.clone() else {
                new_state.toast_error("UI update sender is unavailable");
                return EventCommandLoopAction::ContinueCommand;
            };
            let operation = LocalTerminalOperationOwner::submit(
                app_core_for_events.clone(),
                tasks_for_events.clone(),
                update_tx,
                OperationId::create_channel(),
                aura_app::ui_contract::SemanticOperationKind::CreateChannel,
            );
            (cb.chat.on_create_channel)(
                name,
                topic,
                members.into_iter().map(|id| id.to_string()).collect(),
                threshold_k.get(),
                operation,
            );
        }
        DispatchCommand::SetChannelTopic { channel_id, topic } => {
            (cb.chat.on_set_topic)(channel_id.to_string(), topic);
        }
        DispatchCommand::DeleteChannel { channel_id } => {
            (cb.chat.on_close_channel)(channel_id.to_string());
        }

        // === Contacts Screen Commands ===
        DispatchCommand::UpdateNickname {
            contact_id,
            nickname,
        } => {
            (cb.contacts.on_update_nickname)(contact_id.to_string(), nickname);
        }
        DispatchCommand::OpenContactNicknameModal => {
            let idx = new_state.contacts.selected_index;
            {
                let guard = shared_contacts_for_dispatch.read();
                if let Some(contact) = guard.get(idx) {
                    // nickname is already populated with nickname_suggestion if empty (see Contact::from)
                    let modal_state = crate::tui::state::NicknameModalState::for_contact(
                        &contact.id,
                        &contact.nickname,
                    )
                    .with_suggestion(contact.nickname_suggestion.clone());
                    new_state.modal_queue.enqueue(
                        crate::tui::state::QueuedModal::ContactsNickname(modal_state),
                    );
                } else {
                    new_state.toast_error("No contact selected");
                }
            }
        }
        DispatchCommand::InviteLanPeer => {
            let idx = new_state.contacts.lan_selected_index;
            {
                let guard = shared_discovered_peers_for_dispatch.read();
                if let Some(peer) = guard.get(idx) {
                    let authority_id = peer.authority_id.to_string();
                    let address = peer.address.clone();
                    if address.is_empty() {
                        new_state.toast_error("Selected peer has no LAN address");
                    } else {
                        (cb.contacts.on_invite_lan_peer)(authority_id, address);
                    }
                } else {
                    new_state.toast_error("No LAN peer selected");
                }
            }
        }
        DispatchCommand::StartChat => {
            let idx = new_state.contacts.selected_index;
            {
                let guard = shared_contacts_for_dispatch.read();
                if let Some(contact) = guard.get(idx) {
                    (cb.contacts.on_start_chat)(contact.id.clone());
                } else {
                    new_state.toast_error("No contact selected");
                }
            }
        }
        DispatchCommand::InviteSelectedContactToChannel => {
            let contact_idx = new_state.contacts.selected_index;
            let channel_idx = new_state.chat.selected_channel;
            let contacts = shared_contacts_for_dispatch.read().clone();
            let channels = shared_channels_for_dispatch.read().clone();
            let Some(contact) = contacts.get(contact_idx) else {
                new_state.toast_error("No contact selected");
                return EventCommandLoopAction::ContinueCommand;
            };
            let Some(channel) = channels.get(channel_idx) else {
                new_state.toast_error("No channel selected");
                return EventCommandLoopAction::ContinueCommand;
            };
            let selected_binding = selected_channel_binding_for_events
                .read()
                .clone()
                .filter(|binding| binding.channel_id == channel.id);
            let context_id = selected_binding.and_then(|binding| binding.context_id);
            let Some(context_id) = context_id else {
                new_state.toast_error(format!(
                    "Selected channel lacks authoritative context: {}",
                    channel.id
                ));
                return EventCommandLoopAction::ContinueCommand;
            };
            let Some(update_tx) = update_tx_for_events.clone() else {
                new_state.toast_error("UI update sender is unavailable");
                return EventCommandLoopAction::ContinueCommand;
            };
            let operation = WorkflowHandoffOperationOwner::submit(
                app_core_for_events.clone(),
                tasks_for_events.clone(),
                update_tx,
                OperationId::invitation_create(),
                SemanticOperationKind::InviteActorToChannel,
            );
            new_state.clear_runtime_fact_kind(RuntimeEventKind::PendingHomeInvitationReady);
            (cb.contacts.on_invite_to_channel)(
                contact.id.clone(),
                channel.id.clone(),
                Some(context_id),
                operation,
            );
        }
        DispatchCommand::InviteActorToChannel {
            authority_id,
            channel_id,
        } => {
            let channels = shared_channels_for_dispatch.read().clone();
            let channel_id_string = channel_id.clone();
            let Some(channel) = channels
                .iter()
                .find(|channel| channel.id == channel_id_string)
            else {
                new_state.toast_error(format!(
                    "Selected channel is stale or unavailable: {channel_id}"
                ));
                return EventCommandLoopAction::ContinueCommand;
            };
            let selected_binding = selected_channel_binding_for_events
                .read()
                .clone()
                .filter(|binding| binding.channel_id == channel.id);
            let context_id = selected_binding.and_then(|binding| binding.context_id);
            let Some(context_id) = context_id else {
                new_state.toast_error(format!(
                    "Selected channel lacks authoritative context: {}",
                    channel.id
                ));
                return EventCommandLoopAction::ContinueCommand;
            };
            let Some(update_tx) = update_tx_for_events.clone() else {
                new_state.toast_error("UI update sender is unavailable");
                return EventCommandLoopAction::ContinueCommand;
            };
            let operation = WorkflowHandoffOperationOwner::submit(
                app_core_for_events.clone(),
                tasks_for_events.clone(),
                update_tx,
                OperationId::invitation_create(),
                SemanticOperationKind::InviteActorToChannel,
            );
            new_state.clear_runtime_fact_kind(RuntimeEventKind::PendingHomeInvitationReady);
            (cb.contacts.on_invite_to_channel)(
                authority_id.to_string(),
                channel.id.clone(),
                Some(context_id),
                operation,
            );
        }
        DispatchCommand::RemoveContact { contact_id } => {
            let Some(update_tx) = update_tx_for_events.clone() else {
                new_state.toast_error("UI update sender is unavailable");
                return EventCommandLoopAction::ContinueCommand;
            };
            let operation = LocalTerminalOperationOwner::submit(
                app_core_for_events.clone(),
                tasks_for_events.clone(),
                update_tx,
                OperationId::remove_contact(),
                SemanticOperationKind::RemoveContact,
            );
            (cb.contacts.on_remove_contact)(contact_id.to_string(), operation);
        }
        DispatchCommand::OpenRemoveContactModal => {
            let idx = new_state.contacts.selected_index;
            {
                let guard = shared_contacts_for_dispatch.read();
                if let Some(contact) = guard.get(idx) {
                    // Get display name for confirmation message
                    let display_name = if !contact.nickname.is_empty() {
                        contact.nickname.clone()
                    } else if let Some(s) = &contact.nickname_suggestion {
                        s.clone()
                    } else {
                        let short = contact.id.chars().take(8).collect::<String>();
                        format!("{short}...")
                    };

                    // Show confirmation modal
                    new_state
                        .modal_queue
                        .enqueue(crate::tui::state::QueuedModal::Confirm {
                            title: "Remove Contact".to_string(),
                            message: format!("Are you sure you want to remove \"{display_name}\"?"),
                            on_confirm: Some(crate::tui::state::ConfirmAction::RemoveContact {
                                contact_id: contact.id.clone().into(),
                            }),
                        });
                } else {
                    new_state.toast_error("No contact selected");
                }
            }
        }
        DispatchCommand::SelectContactByIndex { index } => {
            // Generic contact selection by index
            // This is used by ContactSelect modal - map index to contact_id
            tracing::info!("Contact selected by index: {}", index);
            // Dismiss the modal after selection
            new_state.modal_queue.dismiss();
        }

        // === Invitations Screen Commands ===
        DispatchCommand::AcceptInvitation => {
            let selected = read_selected_notification(
                new_state.notifications.selected_index,
                &shared_invitations_for_dispatch,
                &shared_pending_requests_for_dispatch,
            );
            if let Some(NotificationSelection::ReceivedInvitation(invitation_id)) = selected {
                if let Some(update_tx) = update_tx_for_dispatch.clone() {
                    let operation = WorkflowHandoffOperationOwner::submit(
                        app_ctx_for_dispatch.app_core.raw().clone(),
                        app_ctx_for_dispatch.tasks(),
                        update_tx,
                        OperationId::invitation_accept(),
                        semantic_accept_kind_for_invitation(
                            &shared_invitations_for_dispatch,
                            &invitation_id,
                        ),
                    );
                    new_state.clear_runtime_fact_kind(RuntimeEventKind::ContactLinkReady);
                    (cb.invitations.on_accept)(invitation_id, operation);
                } else {
                    new_state.toast_error("UI update sender is unavailable");
                }
            } else {
                new_state.toast_error("Select a received invitation to accept");
            }
        }
        DispatchCommand::DeclineInvitation => {
            let selected = read_selected_notification(
                new_state.notifications.selected_index,
                &shared_invitations_for_dispatch,
                &shared_pending_requests_for_dispatch,
            );
            if let Some(NotificationSelection::ReceivedInvitation(invitation_id)) = selected {
                let Some(update_tx) = update_tx_for_events.clone() else {
                    new_state.toast_error("UI update sender is unavailable");
                    return EventCommandLoopAction::ContinueCommand;
                };
                let operation = LocalTerminalOperationOwner::submit(
                    app_core_for_events.clone(),
                    tasks_for_events.clone(),
                    update_tx,
                    OperationId::invitation_decline(),
                    SemanticOperationKind::DeclineInvitation,
                );
                (cb.invitations.on_decline)(invitation_id, operation);
            } else {
                new_state.toast_error("Select a received invitation to decline");
            }
        }
        DispatchCommand::CreateInvitation {
            receiver_id,
            invitation_type,
            message,
            ttl_secs,
        } => {
            let Some(update_tx) = update_tx_for_events.clone() else {
                new_state.toast_error("UI update sender is unavailable");
                return EventCommandLoopAction::ContinueCommand;
            };
            let kind = match invitation_type {
                InvitationKind::Contact => SemanticOperationKind::CreateContactInvitation,
                InvitationKind::Guardian => SemanticOperationKind::CreateContactInvitation,
                InvitationKind::Channel => SemanticOperationKind::InviteActorToChannel,
            };
            let operation = LocalTerminalOperationOwner::submit(
                app_core_for_events.clone(),
                tasks_for_events.clone(),
                update_tx,
                OperationId::invitation_create(),
                kind,
            );
            new_state.clear_runtime_fact_kind(RuntimeEventKind::InvitationCodeReady);
            (cb.invitations.on_create)(
                receiver_id,
                invitation_type.as_str().to_owned(),
                message,
                ttl_secs,
                operation,
            );
        }
        DispatchCommand::ImportInvitation { code } => {
            let Some(update_tx) = update_tx_for_events.clone() else {
                new_state.toast_error("UI update sender is unavailable");
                return EventCommandLoopAction::ContinueCommand;
            };
            let operation = WorkflowHandoffOperationOwner::submit(
                app_core_for_events.clone(),
                tasks_for_events.clone(),
                update_tx,
                OperationId::invitation_accept(),
                SemanticOperationKind::AcceptContactInvitation,
            );
            new_state.clear_runtime_fact_kind(RuntimeEventKind::ContactLinkReady);
            (cb.invitations.on_import)(code, operation);
        }
        DispatchCommand::ExportInvitation => {
            let selected = read_selected_notification(
                new_state.notifications.selected_index,
                &shared_invitations_for_dispatch,
                &shared_pending_requests_for_dispatch,
            );
            if let Some(NotificationSelection::SentInvitation(invitation_id)) = selected {
                (cb.invitations.on_export)(invitation_id);
            } else {
                new_state.toast_error("Select a sent invitation to export");
            }
        }
        DispatchCommand::RevokeInvitation { invitation_id } => {
            let Some(update_tx) = update_tx_for_events.clone() else {
                new_state.toast_error("UI update sender is unavailable");
                return EventCommandLoopAction::ContinueCommand;
            };
            let operation = LocalTerminalOperationOwner::submit(
                app_core_for_events.clone(),
                tasks_for_events.clone(),
                update_tx,
                OperationId::invitation_revoke(),
                SemanticOperationKind::RevokeInvitation,
            );
            (cb.invitations.on_revoke)(invitation_id.to_string(), operation);
        }

        // === Recovery And Ceremony Commands ===
        cmd @ (DispatchCommand::StartRecovery
        | DispatchCommand::ApproveRecovery
        | DispatchCommand::StartGuardianCeremony { .. }
        | DispatchCommand::StartMfaCeremony { .. }
        | DispatchCommand::CancelGuardianCeremony { .. }
        | DispatchCommand::CancelKeyRotationCeremony { .. }) => {
            return handle_recovery_and_ceremonies_dispatch(cmd, new_state, event_ctx);
        }

        // === Settings Screen Commands ===
        DispatchCommand::UpdateNicknameSuggestion {
            nickname_suggestion,
        } => {
            (cb.settings.on_update_nickname_suggestion)(nickname_suggestion);
        }
        DispatchCommand::UpdateMfaPolicy { policy } => {
            (cb.settings.on_update_mfa)(policy);
        }
        DispatchCommand::AddDevice {
            name,
            invitee_authority_id,
        } => {
            let Some(update_tx) = update_tx_for_events.clone() else {
                new_state.toast_error("UI update sender is unavailable");
                return EventCommandLoopAction::ContinueCommand;
            };
            let operation = LocalTerminalOperationOwner::submit(
                app_core_for_events.clone(),
                tasks_for_events.clone(),
                update_tx,
                OperationId::device_enrollment(),
                SemanticOperationKind::StartDeviceEnrollment,
            );
            (cb.settings.on_add_device)(name, invitee_authority_id, operation);
        }
        DispatchCommand::RemoveDevice { device_id } => {
            (cb.settings.on_remove_device)(device_id);
        }
        DispatchCommand::ImportDeviceEnrollmentOnMobile { code } => {
            let Some(update_tx) = update_tx_for_events.clone() else {
                new_state.toast_error("UI update sender is unavailable");
                return EventCommandLoopAction::ContinueCommand;
            };
            let operation = LocalTerminalOperationOwner::submit(
                app_core_for_events.clone(),
                tasks_for_events.clone(),
                update_tx,
                OperationId::device_enrollment(),
                SemanticOperationKind::ImportDeviceEnrollmentCode,
            );
            (cb.settings.on_import_device_enrollment_on_mobile)(code, operation);
        }
        DispatchCommand::OpenAuthorityPicker => {
            // Build list of authorities from app-global state
            let authorities = new_state.authorities.clone();
            if authorities.len() <= 1 {
                new_state.toast_info("Only one authority available");
            } else {
                // Convert authorities to contact-like format for picker
                let contacts: Vec<(crate::tui::state::AuthorityRef, String)> = authorities
                    .iter()
                    .map(|a| {
                        (
                            a.id.clone().into(),
                            format!("{} ({})", a.nickname_suggestion, a.short_id),
                        )
                    })
                    .collect();

                let modal_state = crate::tui::state::ContactSelectModalState::single(
                    "Select Authority",
                    contacts,
                );
                new_state
                    .modal_queue
                    .enqueue(crate::tui::state::QueuedModal::AuthorityPicker(modal_state));
            }
        }
        DispatchCommand::SwitchAuthority { authority_id } => {
            let authority_id_str = authority_id.to_string();
            if let Some(idx) = new_state
                .authorities
                .iter()
                .position(|a| a.id == authority_id_str)
            {
                let nickname = new_state.authorities.get(idx).and_then(|auth| {
                    if auth.nickname_suggestion.trim().is_empty() {
                        None
                    } else {
                        Some(auth.nickname_suggestion.clone())
                    }
                });
                new_state.current_authority_index = idx;
                app_ctx_for_dispatch.request_authority_switch(authority_id, nickname.clone());
                new_state.modal_queue.dismiss();
                new_state.toast_info("Reloading selected authority");
                new_state.should_exit = true;
            } else {
                new_state.toast_error("Authority not found");
            }
        }
        // Note: Threshold/guardian changes now use OpenGuardianSetup
        // which is handled above with the guardian ceremony commands.

        // === Neighborhood Screen Commands ===
        DispatchCommand::EnterHome => {
            let idx = new_state.neighborhood.grid.current();
            {
                let guard = shared_neighborhood_homes_for_dispatch.read();
                if let Some(home_id) = guard.get(idx) {
                    // Keep entered_home_id authoritative as a real home ID.
                    // The state-machine layer sets an index sentinel first.
                    new_state.neighborhood.entered_home_id = Some(home_id.clone());
                    // Default to Limited-level traversal depth
                    (cb.neighborhood.on_enter_home)(
                        home_id.clone(),
                        new_state.neighborhood.enter_depth,
                    );
                } else {
                    new_state.toast_error("No home selected");
                }
            }
        }
        DispatchCommand::GoHome => {
            (cb.neighborhood.on_go_home)();
        }
        DispatchCommand::BackToLimited => {
            (cb.neighborhood.on_back_to_limited)();
        }
        DispatchCommand::OpenHomeCreate => {
            // Open home creation modal
            new_state
                .modal_queue
                .enqueue(crate::tui::state::QueuedModal::NeighborhoodHomeCreate(
                    crate::tui::state::HomeCreateModalState::new(),
                ));
        }
        DispatchCommand::OpenModeratorAssignmentModal => {
            let contacts = shared_contacts_for_dispatch.read().clone();
            new_state.modal_queue.enqueue(
                crate::tui::state::QueuedModal::NeighborhoodModeratorAssignment(
                    crate::tui::state::ModeratorAssignmentModalState::new(contacts),
                ),
            );
        }
        DispatchCommand::SubmitModeratorAssignment { target_id, assign } => {
            (cb.neighborhood.on_set_moderator)(
                new_state.neighborhood.entered_home_id.clone(),
                target_id.to_string(),
                assign,
            );
            new_state.modal_queue.dismiss();
        }
        DispatchCommand::OpenAccessOverrideModal => {
            let contacts = shared_contacts_for_dispatch.read().clone();
            new_state.modal_queue.enqueue(
                crate::tui::state::QueuedModal::NeighborhoodAccessOverride(
                    crate::tui::state::AccessOverrideModalState::new(contacts),
                ),
            );
        }
        DispatchCommand::SubmitAccessOverride {
            target_id,
            access_level,
        } => {
            new_state.modal_queue.dismiss();
            let app_core = app_core_for_ceremony.clone();
            let update_tx = update_tx_for_ceremony.clone();
            let home_id = new_state.neighborhood.entered_home_id.clone();
            let target_for_toast = target_id.clone();
            let tasks = tasks_for_events.clone();
            tasks.spawn(async move {
                match access_workflows::set_access_override(
                    app_core.raw(),
                    home_id.as_deref(),
                    target_id,
                    access_level.into(),
                )
                .await
                {
                    Ok(()) => {
                        if let Some(tx) = update_tx {
                            let _ = tx.try_send(UiUpdate::ToastAdded(ToastMessage::success(
                                "access-override",
                                format!(
                                    "Access override set for {}: {}",
                                    target_for_toast,
                                    access_level.label()
                                ),
                            )));
                        }
                    }
                    Err(error) => {
                        if let Some(tx) = update_tx {
                            let _ = tx.try_send(UiUpdate::ToastAdded(ToastMessage::error(
                                "access-override",
                                format!("Failed to set access override: {error}"),
                            )));
                        }
                    }
                }
            });
        }
        DispatchCommand::OpenHomeCapabilityConfigModal => {
            new_state.modal_queue.enqueue(
                crate::tui::state::QueuedModal::NeighborhoodCapabilityConfig(
                    crate::tui::state::HomeCapabilityConfigModalState::default(),
                ),
            );
        }
        DispatchCommand::SubmitHomeCapabilityConfig { config } => {
            new_state.modal_queue.dismiss();
            let app_core = app_core_for_ceremony.clone();
            let update_tx = update_tx_for_ceremony.clone();
            let home_id = new_state.neighborhood.entered_home_id.clone();
            let tasks = tasks_for_events.clone();
            tasks.spawn(async move {
                match access_workflows::configure_home_capabilities(
                    app_core.raw(),
                    home_id.as_deref(),
                    &config.full_csv(),
                    &config.partial_csv(),
                    &config.limited_csv(),
                )
                .await
                {
                    Ok(()) => {
                        if let Some(tx) = update_tx {
                            let _ = tx.try_send(UiUpdate::ToastAdded(ToastMessage::success(
                                "capability-config",
                                "Capability config saved",
                            )));
                        }
                    }
                    Err(error) => {
                        if let Some(tx) = update_tx {
                            let _ = tx.try_send(UiUpdate::ToastAdded(ToastMessage::error(
                                "capability-config",
                                format!("Failed to save capability config: {error}"),
                            )));
                        }
                    }
                }
            });
        }
        DispatchCommand::CreateHome { name, description } => {
            (cb.neighborhood.on_create_home)(name, description);
            new_state.modal_queue.dismiss();
        }
        DispatchCommand::CreateNeighborhood { name } => {
            (cb.neighborhood.on_create_neighborhood)(name);
        }
        DispatchCommand::AddSelectedHomeToNeighborhood => {
            let idx = new_state.neighborhood.grid.current();
            {
                let guard = shared_neighborhood_homes_for_dispatch.read();
                if let Some(home_id) = guard.get(idx) {
                    (cb.neighborhood.on_add_home_to_neighborhood)(home_id.clone());
                } else {
                    new_state.toast_error("No home selected");
                }
            }
        }
        DispatchCommand::AddHomeToNeighborhood { target } => {
            (cb.neighborhood.on_add_home_to_neighborhood)(target.as_command_arg());
        }
        DispatchCommand::LinkSelectedHomeOneHopLink => {
            let idx = new_state.neighborhood.grid.current();
            {
                let guard = shared_neighborhood_homes_for_dispatch.read();
                if let Some(home_id) = guard.get(idx) {
                    (cb.neighborhood.on_link_home_one_hop_link)(home_id.clone());
                } else {
                    new_state.toast_error("No home selected");
                }
            }
        }
        DispatchCommand::LinkHomeOneHopLink { target } => {
            (cb.neighborhood.on_link_home_one_hop_link)(target.as_command_arg());
        }

        // === Navigation Commands ===
        DispatchCommand::NavigateTo(_screen) => {
            // Navigation is handled by TuiState directly
            // The state machine already updates the screen
        }
        cmd @ (DispatchCommand::OpenChatTopicModal
        | DispatchCommand::OpenChatInfoModal
        | DispatchCommand::OpenCreateInvitationModal
        | DispatchCommand::OpenGuardianSetup
        | DispatchCommand::OpenMfaSetup
        | DispatchCommand::OpenDeviceSelectModal) => {
            debug_assert!(
                                            false,
                                            "modal-open command should have been handled before shell dispatch routing: {cmd:?}"
                                        );
        }
    }
    EventCommandLoopAction::Handled
}
