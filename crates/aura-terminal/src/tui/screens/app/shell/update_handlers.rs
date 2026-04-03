use super::updates::*;
use super::*;

use aura_app::ui_contract::ChannelBindingWitness;
use aura_app::ui_contract::SemanticOperationKind;

use crate::tui::channel_selection::authoritative_committed_selection;
use crate::tui::components::copy_to_clipboard;
use crate::tui::screens::app::shell::dispatch::{
    format_ui_operation_failure, set_authoritative_operation_state_sanctioned,
};

pub(super) async fn process_ui_update_match(
    update: UiUpdate,
    ctx: &mut UiUpdateContext,
) -> UiUpdateLoopAction {
    let show_setup = ctx.show_setup;
    let nickname_suggestion_state = &mut ctx.nickname_suggestion_state;
    let should_exit = &mut ctx.should_exit;
    let app_core = ctx.app_ctx.app_core.clone();
    let app_ctx_for_updates = ctx.app_ctx.clone();
    let bootstrap_handoff_tx = &ctx.bootstrap_handoff_tx;
    let io_ctx = ctx.app_ctx.io_context();
    let bg_shutdown = &ctx.bg_shutdown;
    let tui = &mut ctx.tui;
    let tasks_for_updates = ctx.tasks_for_updates.clone();
    let shared_contacts_for_updates = &ctx.shared_contacts_for_updates;
    let shared_channels_for_updates = &ctx.shared_channels_for_updates;
    let shared_devices_for_updates = &ctx.shared_devices_for_updates;
    let shared_messages_for_updates = &ctx.shared_messages_for_updates;
    let tui_selected_for_updates = &ctx.tui_selected_for_updates;
    let ready_join_channel_instances_for_updates = &ctx.ready_join_channel_instances_for_updates;

    macro_rules! enqueue_toast {
        ($msg:expr, $level:expr) => {{
            tui.with_mut(|state| {
                let toast_id = state.next_toast_id;
                state.next_toast_id += 1;
                let toast = crate::tui::state::QueuedToast::new(toast_id, $msg, $level);
                state.toast_queue.enqueue(toast);
            });
        }};
    }

    match update {
        // =========================================================================
        // Settings updates
        // =========================================================================
        UiUpdate::NicknameSuggestionChanged(name) => {
            nickname_suggestion_state.set(name);
        }
        UiUpdate::MfaPolicyChanged(_policy) => {
            // Settings screen renders from SETTINGS_SIGNAL; no local state update.
        }
        UiUpdate::ThresholdChanged { k: _, n: _ } => {
            // Settings screen renders from SETTINGS_SIGNAL; no local state update.
        }
        UiUpdate::DeviceAdded(_device) => {
            // Settings screen renders from SETTINGS_SIGNAL; no local state update.
        }
        UiUpdate::DeviceRemoved { device_id: _ } => {
            // Settings screen renders from SETTINGS_SIGNAL; no local state update.
        }
        UiUpdate::AuthoritiesUpdated {
            authorities,
            current_index,
        } => {
            tui.with_mut(|state| {
                state.authorities = authorities.clone();
                state.current_authority_index =
                    current_index.min(state.authorities.len().saturating_sub(1));
            });
        }
        UiUpdate::RuntimeBootstrapFinalized => {
            let should_clear = tui.read_clone().pending_runtime_bootstrap;
            if should_clear {
                tui.with_mut(|state| {
                    state.pending_runtime_bootstrap = false;
                });
            }
        }
        UiUpdate::DeviceEnrollmentStarted {
            ceremony_id,
            nickname_suggestion,
            enrollment_code,
            pending_epoch: _,
            device_id: _,
        } => {
            let _ = copy_to_clipboard(&enrollment_code);
            tui.with_mut(|state| {
                state.settings.last_device_enrollment_code = enrollment_code.clone();
                state.upsert_runtime_fact(RuntimeFact::DeviceEnrollmentCodeReady {
                    device_name: Some(nickname_suggestion.clone()),
                    code_len: Some(enrollment_code.len()),
                    code: Some(enrollment_code.clone()),
                });
                if state.settings.pending_mobile_enrollment_autofill {
                    state.settings.pending_mobile_enrollment_autofill = false;
                    state.modal_queue.update_active(|modal| {
                        if let crate::tui::state::QueuedModal::SettingsDeviceImport(ref mut s) =
                            modal
                        {
                            s.code = enrollment_code.clone();
                        }
                    });
                } else {
                    state.modal_queue.enqueue(
                        crate::tui::state::QueuedModal::SettingsDeviceEnrollment(
                            crate::tui::state::DeviceEnrollmentCeremonyModalState::started(
                                ceremony_id,
                                nickname_suggestion,
                                enrollment_code,
                            ),
                        ),
                    );
                }
            });
        }
        UiUpdate::KeyRotationCeremonyStatus {
            ceremony_id,
            kind,
            accepted_count,
            total_count,
            threshold,
            is_complete,
            has_failed,
            accepted_participants,
            error_message,
            pending_epoch,
            agreement_mode,
            reversion_risk,
        } => {
            let mut toast: Option<(String, crate::tui::state::ToastLevel)> = None;
            let mut dismiss_ceremony_started_toast = false;
            let mut handled_device_enrollment_modal = false;
            tui.with_mut(|state| {
                                let mut dismiss_modal = false;

                                state.modal_queue.update_active(|modal| {
                                    if let crate::tui::state::QueuedModal::SettingsDeviceEnrollment(ref mut s) = modal {
                                        handled_device_enrollment_modal = true;
                                        if s.ceremony.ceremony_id.as_deref() == Some(ceremony_id.as_str()) {
                                            s.update_from_status(
                                                accepted_count,
                                                total_count,
                                                threshold,
                                                is_complete,
                                                has_failed,
                                                error_message.clone(),
                                                pending_epoch,
                                                agreement_mode,
                                                reversion_risk,
                                            );

                                            if has_failed {
                                                toast = Some((
                                                    error_message
                                                        .clone()
                                                        .unwrap_or_else(|| "Device enrollment failed".to_string()),
                                                    crate::tui::state::ToastLevel::Error,
                                                ));
                                            } else if is_complete {
                                                dismiss_modal = true;
                                                toast = Some((
                                                    "Device enrollment complete".to_string(),
                                                    crate::tui::state::ToastLevel::Success,
                                                ));
                                                let app_core = app_core.raw().clone();
                                                let tasks = tasks_for_updates.clone();
                                                tasks.spawn(async move {
                                                    let _ = refresh_settings_from_runtime(&app_core).await;
                                                });
                                            }
                                        }
                                    } else if let crate::tui::state::QueuedModal::GuardianSetup(ref mut s) = modal {
                                        if matches!(
                                            s.step(),
                                            crate::tui::state::GuardianSetupStep::CeremonyInProgress
                                        ) {
                                            // Ensure ceremony id is set for cancel UX.
                                            s.ensure_ceremony_id(ceremony_id.clone());

                                            s.update_ceremony_from_status(
                                                accepted_count,
                                                total_count,
                                                threshold,
                                                is_complete,
                                                has_failed,
                                                error_message.clone(),
                                                pending_epoch,
                                                agreement_mode,
                                                reversion_risk,
                                            );

                                            // Update per-guardian responses based on accepted participants.
                                            use aura_core::threshold::ParticipantIdentity;
                                            let accepted_guardians: Vec<String> = accepted_participants
                                                .iter()
                                                .filter_map(|p| match p {
                                                    ParticipantIdentity::Guardian(id) => Some(id.to_string()),
                                                    _ => None,
                                                })
                                                .collect();

                                            s.update_responses_from_accepted(&accepted_guardians);

                                            if has_failed {
                                                let msg = error_message
                                                    .clone()
                                                    .unwrap_or_else(|| "Guardian ceremony failed".to_string());
                                                // Return to threshold selection so the user can retry.
                                                s.reset_to_threshold_after_failure();

                                                toast = Some((msg, crate::tui::state::ToastLevel::Error));
                                                dismiss_ceremony_started_toast = true;
                                            } else if is_complete {
                                                dismiss_modal = true;
                                                toast = Some((
                                                    match kind {
                                                        aura_app::ui::types::CeremonyKind::GuardianRotation => format!(
                                                            "Guardian ceremony complete! {threshold}-of-{total_count} committed"
                                                        ),
                                                        aura_app::ui::types::CeremonyKind::DeviceEnrollment => {
                                                            "Device enrollment complete".to_string()
                                                        }
                                                        aura_app::ui::types::CeremonyKind::DeviceRemoval => {
                                                            "Device removal complete".to_string()
                                                        }
                                                        aura_app::ui::types::CeremonyKind::DeviceRotation => {
                                                            format!(
                                                                "Device threshold ceremony complete ({threshold}-of-{total_count})"
                                                            )
                                                        }
                                                        aura_app::ui::types::CeremonyKind::Recovery => {
                                                            "Recovery ceremony complete".to_string()
                                                        }
                                                        aura_app::ui::types::CeremonyKind::Invitation => {
                                                            "Invitation ceremony complete".to_string()
                                                        }
                                                        aura_app::ui::types::CeremonyKind::RendezvousSecureChannel => {
                                                            "Rendezvous ceremony complete".to_string()
                                                        }
                                                        aura_app::ui::types::CeremonyKind::OtaActivation => {
                                                            "OTA activation ceremony complete".to_string()
                                                        }
                                                    },
                                                    crate::tui::state::ToastLevel::Success,
                                                ));
                                                dismiss_ceremony_started_toast = true;
                                                if matches!(
                                                    kind,
                                                    aura_app::ui::types::CeremonyKind::DeviceEnrollment
                                                        | aura_app::ui::types::CeremonyKind::DeviceRemoval
                                                        | aura_app::ui::types::CeremonyKind::DeviceRotation
                                                ) {
                                                    let app_core = app_core.raw().clone();
                                                    let tasks = tasks_for_updates.clone();
                                                    tasks.spawn(async move {
                                                        let _ = refresh_settings_from_runtime(&app_core).await;
                                                    });
                                                }
                                            }
                                        }
                                    } else if let crate::tui::state::QueuedModal::MfaSetup(ref mut s) = modal {
                                        if matches!(
                                            s.step(),
                                            crate::tui::state::GuardianSetupStep::CeremonyInProgress
                                        ) {
                                            s.ensure_ceremony_id(ceremony_id.clone());

                                            s.update_ceremony_from_status(
                                                accepted_count,
                                                total_count,
                                                threshold,
                                                is_complete,
                                                has_failed,
                                                error_message.clone(),
                                                pending_epoch,
                                                agreement_mode,
                                                reversion_risk,
                                            );

                                            use aura_core::threshold::ParticipantIdentity;
                                            let accepted_devices: Vec<String> = accepted_participants
                                                .iter()
                                                .filter_map(|p| match p {
                                                    ParticipantIdentity::Device(id) => Some(id.to_string()),
                                                    _ => None,
                                                })
                                                .collect();

                                            s.update_responses_from_accepted(&accepted_devices);

                                            if has_failed {
                                                let msg = error_message
                                                    .clone()
                                                    .unwrap_or_else(|| "Multifactor ceremony failed".to_string());
                                                s.reset_to_threshold_after_failure();

                                                toast = Some((msg, crate::tui::state::ToastLevel::Error));
                                                dismiss_ceremony_started_toast = true;
                                            } else if is_complete {
                                                dismiss_modal = true;
                                                toast = Some((
                                                    format!(
                                                        "Multifactor ceremony complete! {threshold}-of-{total_count} committed"
                                                    ),
                                                    crate::tui::state::ToastLevel::Success,
                                                ));
                                                dismiss_ceremony_started_toast = true;
                                            }
                                        }
                                    }
                                });

                                if dismiss_modal {
                                    state.modal_queue.dismiss();
                                }
                                if dismiss_ceremony_started_toast {
                                    state.toast_queue.dismiss();
                                }
                            });

            if !handled_device_enrollment_modal
                && matches!(kind, aura_app::ui::types::CeremonyKind::DeviceEnrollment)
                && (is_complete || has_failed)
            {
                let app_core = app_core.raw().clone();
                let tasks = tasks_for_updates.clone();
                tasks.spawn(async move {
                    let _ = refresh_settings_from_runtime(&app_core).await;
                });
                if is_complete {
                    toast = Some((
                        "Device enrollment complete".to_string(),
                        crate::tui::state::ToastLevel::Success,
                    ));
                } else if has_failed {
                    toast = Some((
                        error_message.unwrap_or_else(|| "Device enrollment failed".to_string()),
                        crate::tui::state::ToastLevel::Error,
                    ));
                }
            }

            if let Some((msg, level)) = toast {
                enqueue_toast!(msg, level);
            }
        }

        // =========================================================================
        // Toast notifications
        // =========================================================================
        UiUpdate::ToastAdded(toast) => {
            let level = toast.level.queue_level();
            enqueue_toast!(toast.message, level);
        }
        UiUpdate::ToastDismissed { toast_id: _ } => {
            // Dismiss from queue (FIFO, ignores ID).
            tui.with_mut(|state| {
                state.toast_queue.dismiss();
            });
        }
        UiUpdate::ToastsCleared => {
            tui.with_mut(|state| {
                state.toast_queue.clear();
            });
        }

        // =========================================================================
        // Chat / messaging
        // =========================================================================
        UiUpdate::MessageSent { channel, content } => {
            let mut appended = false;
            let selected_channel = tui_selected_for_updates.read().clone();
            let state_selected_channel = shared_channels_for_updates
                .read()
                .get(tui.read_clone().chat.selected_channel)
                .map(|candidate| candidate.id.clone());
            {
                let mut messages = shared_messages_for_updates.write();
                let should_append = selected_channel
                    .as_ref()
                    .map(CommittedChannelSelection::channel_id)
                    == Some(channel.as_str())
                    || state_selected_channel.as_deref() == Some(channel.as_str());
                if should_append {
                    let already_visible = messages.iter().any(|message| {
                        message.channel_id == channel.as_str()
                            && message.is_own
                            && message.content == content
                    });
                    if !already_visible {
                        let message_idx = messages.len();
                        messages.push(crate::tui::types::Message::sending(
                            format!("local-accepted-{channel}-{message_idx}"),
                            channel,
                            "You",
                            content,
                        ));
                        appended = true;
                    }
                }
            }
            // Auto-scroll to bottom (show latest messages including the one just sent)
            tui.with_mut(|state| {
                if appended {
                    state.chat.message_count = state.chat.message_count.saturating_add(1);
                }
                state.chat.message_scroll = 0;
            });
        }
        UiUpdate::MessageRetried { message_id: _ } => {
            enqueue_toast!(
                "Retrying message…".to_string(),
                crate::tui::state::ToastLevel::Info
            );
        }
        UiUpdate::ChannelSelected(binding) => {
            let selected_channel = shared_channels_for_updates
                .read()
                .iter()
                .enumerate()
                .find_map(|(idx, channel)| {
                    (channel.id == binding.channel_id).then_some((idx, channel.clone()))
                });
            *tui_selected_for_updates.write() =
                Some(CommittedChannelSelection::from_binding(&binding));
            if let Some((idx, _channel)) = selected_channel {
                tui.with_mut(|state| {
                    state.chat.selected_channel = idx;
                    state.chat.message_scroll = 0;
                });
            }
            let selected_binding = tui_selected_for_updates
                .read()
                .clone()
                .map(|selection| selection.binding().clone());
            if let Some(binding) = selected_binding {
                complete_ready_join_binding_submissions(
                    ready_join_channel_instances_for_updates,
                    &binding,
                )
                .await;
            }
        }
        UiUpdate::ChannelCreated {
            operation_instance_id,
            channel_id,
            context_id,
            name,
        } => {
            *tui_selected_for_updates.write() = Some(CommittedChannelSelection::from_binding(
                &ChannelBindingWitness::new(channel_id.clone(), context_id.clone()),
            ));
            {
                let mut channels = shared_channels_for_updates.write();
                if let Some(channel) = channels.iter_mut().find(|channel| channel.id == channel_id)
                {
                    if channel.context_id.is_none() {
                        channel.context_id = context_id.clone();
                    }
                } else {
                    let mut channel = Channel::new(channel_id.clone(), name.clone());
                    channel.context_id = context_id.clone();
                    channel.member_count = 1;
                    channels.push(channel);
                }
            }
            let selected = shared_channels_for_updates
                .read()
                .iter()
                .position(|channel| channel.id == channel_id);
            if let Some(idx) = selected {
                tui.with_mut(|state| {
                    state.chat.selected_channel = idx;
                    state.chat.message_scroll = 0;
                });
            }
            enqueue_toast!(
                format!("Created '{name}'."),
                crate::tui::state::ToastLevel::Success
            );
            if let Some(instance_id) = operation_instance_id {
                if let Err(error) = complete_pending_semantic_submission(
                    instance_id,
                    ChannelBindingWitness::new(channel_id, context_id).semantic_value(),
                )
                .await
                {
                    tracing::warn!(
                        error = %error,
                        "failed to settle pending create-channel binding submission"
                    );
                }
            }
        }
        UiUpdate::ChatStateUpdated {
            channel_count,
            message_count,
            selected_index,
        } => {
            tui.with_mut(|state| {
                let prev_message_count = state.chat.message_count;
                let was_at_bottom = state.chat.message_scroll == 0;

                state.chat.channel_count = channel_count;
                state.chat.message_count = message_count;

                if channel_count == 0 {
                    state.chat.message_scroll = 0;
                    return;
                }

                let committed_selection = tui_selected_for_updates.read().clone();
                let committed_index = committed_selection.as_ref().and_then(|selected| {
                    shared_channels_for_updates
                        .read()
                        .iter()
                        .position(|channel| channel.id == selected.channel_id())
                });

                if let Some(idx) = committed_index {
                    state.chat.selected_channel = idx;
                } else if state.chat.selected_channel >= channel_count {
                    let idx = clamp_list_index(selected_index.unwrap_or(0), channel_count);
                    state.chat.selected_channel = idx;
                    state.chat.message_scroll = 0;
                }

                *tui_selected_for_updates.write() = shared_channels_for_updates
                    .read()
                    .get(state.chat.selected_channel)
                    .map(authoritative_committed_selection);

                // Auto-scroll to bottom when new messages arrive, but only if
                // user was already at the bottom (hasn't scrolled up to read history)
                let new_messages_arrived = message_count > prev_message_count;
                if new_messages_arrived && was_at_bottom {
                    state.chat.message_scroll = 0;
                }

                // Clamp scroll to valid range
                let max_scroll = message_count.saturating_sub(18);
                if state.chat.message_scroll > max_scroll {
                    state.chat.message_scroll = max_scroll;
                }
            });
            let selected_binding = tui_selected_for_updates
                .read()
                .clone()
                .map(|selection| selection.binding().clone());
            if let Some(binding) = selected_binding {
                complete_ready_join_binding_submissions(
                    ready_join_channel_instances_for_updates,
                    &binding,
                )
                .await;
            }
        }
        UiUpdate::TopicSet {
            channel: _,
            topic: _,
        } => {
            // CHAT_SIGNAL should reflect updated topic; no extra work.
        }
        UiUpdate::NeighborhoodStateUpdated { message_count } => {
            tui.with_mut(|state| {
                let prev_message_count = state.neighborhood.message_count;
                let was_at_bottom = state.neighborhood.message_scroll == 0;

                state.neighborhood.message_count = message_count;

                // Auto-scroll to bottom when new messages arrive, but only if
                // user was already at the bottom (hasn't scrolled up to read history)
                let new_messages_arrived = message_count > prev_message_count;
                if new_messages_arrived && was_at_bottom {
                    state.neighborhood.message_scroll = 0;
                }

                // Clamp scroll to valid range
                let max_scroll = message_count.saturating_sub(18);
                if state.neighborhood.message_scroll > max_scroll {
                    state.neighborhood.message_scroll = max_scroll;
                }
            });
        }
        UiUpdate::ChannelInfoParticipants {
            channel_id,
            participants,
        } => {
            let mapped_participants = {
                let contacts = shared_contacts_for_updates.read();
                participants
                    .iter()
                    .map(|entry| {
                        if entry == "You" {
                            return entry.clone();
                        }
                        if let Some(contact) = contacts.iter().find(|c| c.id == *entry) {
                            if !contact.nickname.is_empty() {
                                return contact.nickname.clone();
                            }
                            if let Some(name) = &contact.nickname_suggestion {
                                return name.clone();
                            }
                        }
                        entry.clone()
                    })
                    .collect::<Vec<_>>()
            };
            tui.with_mut(|state| {
                state.modal_queue.update_active(|modal| {
                    if let crate::tui::state::QueuedModal::ChatInfo(ref mut info) = modal {
                        if info.channel_id == channel_id
                            && (mapped_participants.len() > 1 || info.participants.len() <= 1)
                        {
                            info.participants = mapped_participants.clone();
                        }
                    }
                });
            });
        }

        // =========================================================================
        // Invitations
        // =========================================================================
        UiUpdate::InvitationAccepted { invitation_id: _ } => {
            enqueue_toast!(
                "Invitation accepted".to_string(),
                crate::tui::state::ToastLevel::Success
            );
        }
        UiUpdate::InvitationDeclined { invitation_id: _ } => {
            enqueue_toast!(
                "Invitation declined".to_string(),
                crate::tui::state::ToastLevel::Info
            );
        }
        UiUpdate::InvitationCreated { invitation_code: _ } => {
            enqueue_toast!(
                "Invitation created".to_string(),
                crate::tui::state::ToastLevel::Success
            );
        }
        UiUpdate::InvitationExported {
            code,
            operation_id,
            instance_id,
        } => {
            let source_operation = operation_id
                .clone()
                .unwrap_or_else(OperationId::invitation_create);
            let runtime_code = code.clone();
            tui.with_mut(|state| {
                state.last_exported_invitation_code = Some(runtime_code.clone());
                state.upsert_runtime_fact(RuntimeFact::InvitationCodeReady {
                    receiver_authority_id: None,
                    source_operation,
                    code: Some(runtime_code.clone()),
                });
                let copied = copy_to_clipboard(&runtime_code).is_ok();
                state
                    .modal_queue
                    .enqueue(crate::tui::state::QueuedModal::ContactsCode({
                        let mut modal =
                            crate::tui::state::InvitationCodeModalState::for_code(runtime_code);
                        if copied {
                            modal.set_copied();
                        }
                        modal
                    }));
            });
            if let Some(instance_id) = instance_id {
                if let Err(error) = complete_pending_semantic_submission(
                    instance_id,
                    SemanticCommandValue::ContactInvitationCode { code },
                )
                .await
                {
                    tracing::warn!(
                        error = %error,
                        "failed to settle pending contact invitation submission"
                    );
                }
            }
        }
        UiUpdate::AuthoritativeOperationStatus {
            operation_id,
            status,
            instance_id,
            causality,
        } => {
            if let Some(instance_id) = instance_id.clone() {
                let pending_instance_id = instance_id.clone();
                let is_join_channel = status.kind == SemanticOperationKind::JoinChannel
                    && operation_id == aura_app::ui_contract::OperationId::join_channel();
                let is_failed_or_cancelled = matches!(
                    status.phase,
                    aura_app::ui_contract::SemanticOperationPhase::Failed
                        | aura_app::ui_contract::SemanticOperationPhase::Cancelled
                );
                let is_succeeded = matches!(
                    status.phase,
                    aura_app::ui_contract::SemanticOperationPhase::Succeeded
                );
                if is_join_channel {
                    if is_failed_or_cancelled {
                        ready_join_channel_instances_for_updates
                            .lock()
                            .unwrap()
                            .remove(&instance_id.0);
                        let reason = status
                            .error
                            .as_ref()
                            .and_then(|error| error.detail.clone())
                            .unwrap_or_else(|| "join channel failed".to_string());
                        if let Err(error) =
                            fail_pending_semantic_submission(pending_instance_id.clone(), reason)
                                .await
                        {
                            tracing::warn!(
                                error = %error,
                                "failed to reject pending join-channel binding submission"
                            );
                        }
                    } else if is_succeeded {
                        ready_join_channel_instances_for_updates
                            .lock()
                            .unwrap()
                            .insert(instance_id.0.clone());
                        let selected_binding = {
                            tui_selected_for_updates
                                .read()
                                .clone()
                                .map(|selection| selection.binding().clone())
                        };
                        if let Some(binding) = selected_binding {
                            complete_ready_join_binding_submissions(
                                ready_join_channel_instances_for_updates,
                                &binding,
                            )
                            .await;
                        }
                    }
                }
                let tracks_pending_semantic_value = operation_id
                    == aura_app::ui_contract::OperationId::create_channel()
                    || operation_id == aura_app::ui_contract::OperationId::join_channel()
                    || operation_id == aura_app::ui_contract::OperationId::invitation_create();
                if matches!(
                    status.phase,
                    aura_app::ui_contract::SemanticOperationPhase::Failed
                        | aura_app::ui_contract::SemanticOperationPhase::Cancelled
                ) && tracks_pending_semantic_value
                {
                    let reason = status
                        .error
                        .as_ref()
                        .and_then(|error| error.detail.clone())
                        .unwrap_or_else(|| format!("{} failed", operation_id.0));
                    if let Err(error) =
                        fail_pending_semantic_submission(pending_instance_id, reason).await
                    {
                        tracing::warn!(
                            error = %error,
                            "failed to reject pending semantic submission"
                        );
                    }
                }
            }
            let failure_message = if matches!(
                status.phase,
                aura_app::ui_contract::SemanticOperationPhase::Failed
                    | aura_app::ui_contract::SemanticOperationPhase::Cancelled
            ) {
                status.error.as_ref().map(|error| {
                    let detail = error
                        .detail
                        .clone()
                        .unwrap_or_else(|| format!("{:?}", error.code));
                    match status.kind {
                        SemanticOperationKind::InviteActorToChannel => {
                            format!("Invite to channel failed: {detail}")
                        }
                        SemanticOperationKind::CreateChannel => {
                            format!("Create channel failed: {detail}")
                        }
                        _ => detail,
                    }
                })
            } else {
                None
            };
            let next_state = match status.phase {
                aura_app::ui_contract::SemanticOperationPhase::Failed => OperationState::Failed,
                aura_app::ui_contract::SemanticOperationPhase::Cancelled => OperationState::Failed,
                aura_app::ui_contract::SemanticOperationPhase::Succeeded => {
                    OperationState::Succeeded
                }
                _ => OperationState::Submitting,
            };
            tui.with_mut(|state| {
                set_authoritative_operation_state_sanctioned(
                    state,
                    operation_id,
                    instance_id,
                    causality,
                    next_state,
                );
            });
            if let Some(message) = failure_message {
                tui.with_mut(|state| {
                    state.toast_queue.clear();
                });
                enqueue_toast!(message, crate::tui::state::ToastLevel::Error);
            }
            let export_state = tui.read_clone();
            let app_snapshot = match authoritative_app_snapshot_with_retry(
                                &app_ctx_for_updates,
                                "failed to publish TUI harness snapshot after authoritative operation status update",
                            )
                            .await
                            {
                                Ok(snapshot) => snapshot,
                                Err(error) => {
                                    tracing::warn!(error = %error);
                                    if show_setup {
                                        if let Err(export_error) =
                                            publish_loading_ui_snapshot(&export_state)
                                        {
                                            tracing::warn!(
                                                error = %export_error,
                                                "failed to publish bootstrap loading snapshot after authoritative operation status update"
                                            );
                                        }
                                    }
                                    return UiUpdateLoopAction::ContinueLoop;
                                }
                            };
            let harness_contacts = shared_contacts_for_updates.read().clone();
            let harness_channels = shared_channels_for_updates.read().clone();
            let harness_devices = shared_devices_for_updates.read().clone();
            let harness_messages = shared_messages_for_updates.read().clone();
            if let Err(error) = maybe_export_ui_snapshot(
                &export_state,
                TuiSemanticInputs {
                    app_snapshot: &app_snapshot,
                    contacts: &harness_contacts,
                    settings_devices: &harness_devices,
                    chat_channels: &harness_channels,
                    chat_messages: &harness_messages,
                },
            ) {
                tracing::warn!(
                    error = %error,
                    "failed to publish TUI harness snapshot after authoritative operation status update"
                );
            }
        }
        UiUpdate::InvitationImported { invitation_code: _ } => {
            enqueue_toast!(
                "Invitation imported".to_string(),
                crate::tui::state::ToastLevel::Success
            );
        }

        // =========================================================================
        // Navigation
        // =========================================================================
        UiUpdate::HomeEntered { home_id: _ } => {
            // Navigation/state machine owns the current home selection.
        }
        UiUpdate::NavigatedHome => {
            // Navigation/state machine handles this.
        }
        UiUpdate::NavigatedToLimited => {
            // Navigation/state machine handles this.
        }
        UiUpdate::NavigatedToNeighborhood => {
            // Navigation/state machine handles this.
        }

        // =========================================================================
        // Recovery
        // =========================================================================
        UiUpdate::RecoveryStarted => {
            enqueue_toast!(
                "Recovery process started".to_string(),
                crate::tui::state::ToastLevel::Info
            );
        }
        UiUpdate::GuardianAdded { contact_id: _ } => {
            // RECOVERY_SIGNAL owns guardian state; no local state update.
        }
        UiUpdate::GuardianSelected { contact_id: _ } => {
            // RECOVERY_SIGNAL owns guardian state; no local state update.
        }
        UiUpdate::ApprovalSubmitted { request_id: _ } => {
            enqueue_toast!(
                "Approval submitted".to_string(),
                crate::tui::state::ToastLevel::Success
            );
        }
        UiUpdate::GuardianCeremonyProgress { step: _ } => {
            // Deprecated in favor of `GuardianCeremonyStatus`.
        }
        UiUpdate::GuardianCeremonyStatus {
            ceremony_id,
            accepted_guardians,
            total_count,
            threshold,
            is_complete,
            has_failed,
            error_message,
            pending_epoch,
            agreement_mode,
            reversion_risk,
        } => {
            let mut toast: Option<(String, crate::tui::state::ToastLevel)> = None;
            let mut dismiss_ceremony_started_toast = false;

            tui.with_mut(|state| {
                                let mut dismiss_modal = false;

                                state.modal_queue.update_active(|modal| {
                                    if let crate::tui::state::QueuedModal::GuardianSetup(ref mut s) = modal {
                                        if matches!(
                                            s.step(),
                                            crate::tui::state::GuardianSetupStep::CeremonyInProgress
                                        ) {
                                            s.ensure_ceremony_id(ceremony_id.clone());

                                            s.update_ceremony_from_status(
                                                accepted_guardians.len() as u16,
                                                total_count,
                                                threshold,
                                                is_complete,
                                                has_failed,
                                                error_message.clone(),
                                                pending_epoch,
                                                agreement_mode,
                                                reversion_risk,
                                            );

                                            s.update_responses_from_accepted(&accepted_guardians);

                                            if has_failed {
                                                let msg = error_message
                                                    .clone()
                                                    .unwrap_or_else(|| "Guardian ceremony failed".to_string());
                                                // Return to threshold selection so the user can retry.
                                                s.reset_to_threshold_after_failure();

                                                toast = Some((
                                                    msg,
                                                    crate::tui::state::ToastLevel::Error,
                                                ));
                                                dismiss_ceremony_started_toast = true;
                                            } else if is_complete {
                                                dismiss_modal = true;
                                                toast = Some((
                                                    format!(
                                                        "Guardian ceremony complete! {threshold}-of-{total_count} committed"
                                                    ),
                                                    crate::tui::state::ToastLevel::Success,
                                                ));
                                                dismiss_ceremony_started_toast = true;
                                            }
                                        }
                                    }
                                });

                                if dismiss_modal {
                                    state.modal_queue.dismiss();
                                }
                                if dismiss_ceremony_started_toast {
                                    state.toast_queue.dismiss();
                                }
                            });

            if let Some((msg, level)) = toast {
                enqueue_toast!(msg, level);
            }
        }

        // =========================================================================
        // Contacts
        // =========================================================================
        UiUpdate::ContactCountChanged(count) => {
            let needs_update = {
                let state = tui.read_clone();
                state.contacts.contact_count != count
                    || state.contacts.selected_index
                        != clamp_list_index(state.contacts.selected_index, count)
            };
            if needs_update {
                tui.with_mut(|state| {
                    state.contacts.contact_count = count;
                    state.contacts.selected_index =
                        clamp_list_index(state.contacts.selected_index, count);
                });
            }
        }
        UiUpdate::NotificationsCountChanged(count) => {
            let needs_update = {
                let state = tui.read_clone();
                state.notifications.item_count != count
                    || state.notifications.selected_index
                        != clamp_list_index(state.notifications.selected_index, count)
            };
            if needs_update {
                tui.with_mut(|state| {
                    state.notifications.item_count = count;
                    state.notifications.selected_index =
                        clamp_list_index(state.notifications.selected_index, count);
                });
            }
        }
        UiUpdate::SubscriptionDegraded { signal_id, reason } => {
            let changed = tui.with_mut(|state| {
                state.mark_subscription_degraded(signal_id.clone(), reason.clone())
            });
            if changed {
                enqueue_toast!(
                    format!("Signal degraded: {signal_id}"),
                    crate::tui::state::ToastLevel::Warning
                );
            }
        }
        UiUpdate::RuntimeFactsUpdated {
            revision,
            replace_kinds,
            facts,
        } => {
            tui.with_mut(|state| {
                state.apply_runtime_facts_update(revision, replace_kinds, facts);
            });
        }
        UiUpdate::NicknameUpdated {
            contact_id: _,
            nickname: _,
        } => {
            // CONTACTS_SIGNAL owns contact data; no local state update.
        }
        UiUpdate::ChatStarted { contact_id } => {
            // Navigate to Chat screen after starting a direct chat
            tracing::info!("Chat started with contact: {}", contact_id);
            tui.with_mut(|state| {
                state.router.go_to(Screen::Chat);
            });
        }
        UiUpdate::LanPeerInvited { peer_id: _ } => {
            enqueue_toast!(
                "LAN peer invited".to_string(),
                crate::tui::state::ToastLevel::Success
            );
        }
        UiUpdate::LanPeersCountChanged(count) => {
            let needs_update = {
                let state = tui.read_clone();
                state.contacts.lan_peer_count != count
                    || state.contacts.lan_selected_index
                        != clamp_list_index(state.contacts.lan_selected_index, count)
                    || (count == 0
                        && !matches!(
                            state.contacts.list_focus,
                            crate::tui::state::ContactsListFocus::Contacts
                        ))
            };
            if needs_update {
                tui.with_mut(|state| {
                    state.contacts.lan_peer_count = count;
                    if count == 0 {
                        state.contacts.list_focus = crate::tui::state::ContactsListFocus::Contacts;
                    }
                    state.contacts.lan_selected_index =
                        clamp_list_index(state.contacts.lan_selected_index, count);
                });
            }
        }

        // =========================================================================
        // Home operations
        // =========================================================================
        UiUpdate::HomeInviteSent { contact_id: _ } => {
            enqueue_toast!(
                "Invite sent".to_string(),
                crate::tui::state::ToastLevel::Success
            );
        }
        UiUpdate::ModeratorGranted { contact_id: _ } => {
            enqueue_toast!(
                "Moderator granted".to_string(),
                crate::tui::state::ToastLevel::Success
            );
        }
        UiUpdate::ModeratorRevoked { contact_id: _ } => {
            enqueue_toast!(
                "Moderator revoked".to_string(),
                crate::tui::state::ToastLevel::Info
            );
        }

        // =========================================================================
        // Account
        // =========================================================================
        UiUpdate::AccountCreated => {
            let export_state = tui.with_mut(|state| {
                if state
                    .operation_state(&OperationId::account_create())
                    .is_some()
                {
                    set_authoritative_operation_state_sanctioned(
                        state,
                        OperationId::account_create(),
                        None,
                        None,
                        OperationState::Succeeded,
                    );
                }
                if show_setup {
                    state.account_created_bootstrap_handoff_queued();
                    state.should_exit = true;
                } else {
                    state.account_created_queued();
                }
                state.clone()
            });
            if let Err(error) = publish_loading_ui_snapshot(&export_state) {
                tracing::warn!(
                    error = %error,
                    "failed to publish loading snapshot for bootstrap handoff"
                );
            }
            if show_setup {
                request_bootstrap_reload(&io_ctx);
                if let Err(error) = io_ctx.mark_bootstrap_runtime_handoff_committed() {
                    enqueue_toast!(error.to_string(), crate::tui::state::ToastLevel::Error);
                    return UiUpdateLoopAction::ContinueLoop;
                }
                if let Some(tx) = bootstrap_handoff_tx
                    .as_ref()
                    .and_then(|holder| holder.lock().ok().and_then(|mut guard| guard.take()))
                {
                    let _ = tx.send(());
                }
                should_exit.set(true);
                bg_shutdown
                    .read()
                    .store(true, std::sync::atomic::Ordering::Release);
            }
        }

        // =========================================================================
        // Sync
        // =========================================================================
        UiUpdate::SyncStarted => {
            enqueue_toast!("Syncing…".to_string(), crate::tui::state::ToastLevel::Info);
        }
        UiUpdate::SyncCompleted => {
            enqueue_toast!(
                "Sync completed".to_string(),
                crate::tui::state::ToastLevel::Success
            );
        }
        UiUpdate::SyncFailed { error } => {
            enqueue_toast!(
                format!("Sync failed: {}", error),
                crate::tui::state::ToastLevel::Error
            );
        }

        // =========================================================================
        // UI-only errors (domain/runtime errors use ERROR_SIGNAL)
        // =========================================================================
        UiUpdate::OperationFailed { failure } => {
            let message = format_ui_operation_failure(&failure);
            let toast_level = terminal_error_to_toast_level(&failure.error);
            // For account creation, show error in the modal instead of toast.
            if failure.operation.routes_to_account_setup_modal() {
                tui.with_mut(|state| {
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::AccountSetup(ref mut s) = modal {
                            s.set_error(message.clone());
                        }
                    });
                });
            } else {
                enqueue_toast!(message, toast_level);
            }
        }
    }
    UiUpdateLoopAction::Handled
}
