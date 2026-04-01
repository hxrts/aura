use super::*;
use std::collections::HashSet;

use crate::tui::channel_selection::{
    authoritative_committed_selection, strongest_authoritative_binding_for_channel,
};
use crate::tui::screens::app::subscriptions::{
    SharedChannels, SharedContacts, SharedDiscoveredPeers, SharedInvitations, SharedMessages,
    SharedPendingRequests, SharedThreshold,
};
use crate::tui::semantic_lifecycle::{
    CeremonySubmissionOwner, LocalTerminalOperationOwner, WorkflowHandoffOperationOwner,
};
use crate::tui::tasks::UiTaskOwner;
use crate::tui::updates::{publish_ui_update, UiOperationFailure, UiUpdatePublication};
use aura_app::ui_contract::ChannelBindingWitness;
use aura_app::ui_contract::SemanticOperationKind;

use super::dispatch_command_handlers::handle_dispatch_command_match;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum NotificationSelection {
    ReceivedInvitation(String),
    SentInvitation(String),
    RecoveryRequest(String),
}

pub(super) async fn complete_ready_join_binding_submissions(
    ready_instances: &Arc<Mutex<HashSet<String>>>,
    binding: &ChannelBindingWitness,
) {
    if binding.context_id.is_none() {
        return;
    }
    let ready_instance_ids = {
        let mut ready = ready_instances.lock().unwrap();
        ready.drain().collect::<Vec<_>>()
    };
    for instance_id in ready_instance_ids {
        if let Err(error) = complete_pending_semantic_submission(
            aura_app::ui_contract::OperationInstanceId(instance_id),
            binding.clone().semantic_value(),
        )
        .await
        {
            tracing::warn!(
                error = %error,
                "failed to settle pending join-channel binding submission"
            );
        }
    }
}

fn normalized_channel_name_for_harness(value: &str) -> &str {
    value.trim().trim_start_matches('#').trim()
}

pub(super) fn authoritative_binding_for_requested_join(
    command: &HarnessUiCommand,
    channels: &[Channel],
    selected_index: Option<usize>,
    selected_binding: Option<&ChannelBindingWitness>,
) -> Option<SemanticCommandValue> {
    let HarnessUiCommand::JoinChannel { channel_name } = command else {
        return None;
    };
    let requested = normalized_channel_name_for_harness(channel_name);
    if requested.is_empty() {
        return None;
    }

    if let (Some(binding), Some(selected_index)) = (selected_binding, selected_index) {
        if let Some(selected_channel) = channels.get(selected_index) {
            let selected_name = normalized_channel_name_for_harness(&selected_channel.name);
            if selected_channel.id == binding.channel_id
                && selected_name.eq_ignore_ascii_case(requested)
                && binding.context_id.is_some()
            {
                return Some(binding.clone().semantic_value());
            }
        }
    }

    let mut matching_channels = channels.iter().filter(|channel| {
        normalized_channel_name_for_harness(&channel.name).eq_ignore_ascii_case(requested)
    });
    let matched_channel = matching_channels.next()?;
    if matching_channels.next().is_some() {
        return None;
    }
    let context_id = matched_channel.context_id.clone()?;
    Some(ChannelBindingWitness::new(matched_channel.id.clone(), Some(context_id)).semantic_value())
}

pub(super) fn terminal_error_to_toast_level(
    error: &TerminalError,
) -> crate::tui::state::ToastLevel {
    match error.category().toast_severity() {
        aura_app::errors::ToastLevel::Info => crate::tui::state::ToastLevel::Info,
        aura_app::errors::ToastLevel::Success => crate::tui::state::ToastLevel::Success,
        aura_app::errors::ToastLevel::Warning => crate::tui::state::ToastLevel::Warning,
        aura_app::errors::ToastLevel::Error => crate::tui::state::ToastLevel::Error,
    }
}

pub(super) fn format_ui_operation_failure(failure: &UiOperationFailure) -> String {
    let category = failure.error.category();
    format!(
        "[{}] {}: {}. Hint: {}",
        failure.error.code(),
        failure.operation.label(),
        failure.error.message(),
        category.resolution_hint(),
    )
}

pub(super) async fn send_optional_ui_update_required(
    tx: &Option<UiUpdateSender>,
    update: UiUpdate,
) {
    if let Some(tx) = tx {
        let _ = publish_ui_update(tx, update, UiUpdatePublication::RequiredUnordered).await;
    }
}

pub(super) fn submit_local_terminal_operation(
    app_core: Arc<async_lock::RwLock<aura_app::AppCore>>,
    tasks: Arc<UiTaskOwner>,
    update_tx: UiUpdateSender,
    operation_id: OperationId,
    kind: SemanticOperationKind,
) -> LocalTerminalOperationOwner {
    LocalTerminalOperationOwner::submit(app_core, tasks, update_tx, operation_id, kind)
}

pub(super) fn submit_workflow_handoff_operation(
    app_core: Arc<async_lock::RwLock<aura_app::AppCore>>,
    tasks: Arc<UiTaskOwner>,
    update_tx: UiUpdateSender,
    operation_id: OperationId,
    kind: SemanticOperationKind,
) -> WorkflowHandoffOperationOwner {
    WorkflowHandoffOperationOwner::submit(app_core, tasks, update_tx, operation_id, kind)
}

pub(super) fn submit_ceremony_operation(
    app_core: Arc<async_lock::RwLock<aura_app::AppCore>>,
    tasks: Arc<UiTaskOwner>,
    update_tx: UiUpdateSender,
    operation_id: OperationId,
    kind: SemanticOperationKind,
) -> CeremonySubmissionOwner {
    CeremonySubmissionOwner::submit(app_core, tasks, update_tx, operation_id, kind)
}

pub(super) fn set_authoritative_operation_state_sanctioned(
    state: &mut TuiState,
    operation_id: OperationId,
    instance_id: Option<aura_app::ui_contract::OperationInstanceId>,
    causality: Option<aura_app::ui_contract::SemanticOperationCausality>,
    next_state: OperationState,
) {
    state.set_authoritative_operation_state(operation_id, instance_id, causality, next_state);
}

pub(super) fn read_selected_notification(
    selected_index: usize,
    invitations: &std::sync::Arc<parking_lot::RwLock<Vec<Invitation>>>,
    pending_requests: &std::sync::Arc<parking_lot::RwLock<Vec<crate::tui::types::PendingRequest>>>,
) -> Option<NotificationSelection> {
    let invitation_items = invitations
        .read()
        .iter()
        .filter_map(|invitation| {
            let selection = match (invitation.direction, invitation.status) {
                (
                    crate::tui::types::InvitationDirection::Inbound,
                    crate::tui::types::InvitationStatus::Pending,
                ) => Some(NotificationSelection::ReceivedInvitation(
                    invitation.id.clone(),
                )),
                (
                    crate::tui::types::InvitationDirection::Outbound,
                    crate::tui::types::InvitationStatus::Pending,
                ) => Some(NotificationSelection::SentInvitation(invitation.id.clone())),
                _ => None,
            }?;
            Some((invitation.created_at, selection))
        })
        .collect::<Vec<_>>();

    let recovery_items = pending_requests
        .read()
        .iter()
        .map(|request| {
            (
                request.initiated_at,
                NotificationSelection::RecoveryRequest(request.id.clone()),
            )
        })
        .collect::<Vec<_>>();

    let mut notifications = invitation_items;
    notifications.extend(recovery_items);
    notifications.sort_by(|left, right| right.0.cmp(&left.0));

    notifications
        .get(selected_index)
        .map(|(_, selection)| selection.clone())
}

pub(super) fn semantic_accept_kind_for_invitation(
    invitations: &std::sync::Arc<parking_lot::RwLock<Vec<Invitation>>>,
    invitation_id: &str,
) -> SemanticOperationKind {
    invitations
        .read()
        .iter()
        .find(|invitation| invitation.id == invitation_id)
        .map_or(
            SemanticOperationKind::AcceptContactInvitation,
            |invitation| match invitation.invitation_type {
                crate::tui::types::InvitationType::Contact => {
                    SemanticOperationKind::AcceptContactInvitation
                }
                crate::tui::types::InvitationType::Guardian
                | crate::tui::types::InvitationType::Channel => {
                    SemanticOperationKind::AcceptPendingChannelInvitation
                }
            },
        )
}

pub(super) struct HarnessDispatchContext<'a> {
    pub callbacks: &'a Option<CallbackRegistry>,
    pub app_ctx: &'a AppCoreContext,
    pub update_tx: &'a Option<UiUpdateSender>,
    pub shared_invitations: &'a SharedInvitations,
    pub shared_pending_requests: &'a SharedPendingRequests,
    pub shared_contacts: &'a SharedContacts,
    pub shared_channels: &'a SharedChannels,
    pub shared_devices: &'a SharedDevices,
    pub shared_messages: &'a SharedMessages,
    pub last_exported_devices: &'a std::sync::Arc<parking_lot::RwLock<Vec<Device>>>,
    pub selected_channel: &'a SharedCommittedChannelSelection,
}

pub(super) fn execute_harness_followup_command(
    state: &mut TuiState,
    command: TuiCommand,
    ctx: HarnessDispatchContext<'_>,
) -> Result<Option<aura_app::ui_contract::HarnessUiOperationHandle>, String> {
    let callbacks = ctx.callbacks;
    let app_ctx = ctx.app_ctx;
    let update_tx = ctx.update_tx;
    let shared_invitations = ctx.shared_invitations;
    let shared_pending_requests = ctx.shared_pending_requests;
    let shared_contacts = ctx.shared_contacts;
    let shared_channels = ctx.shared_channels;
    let shared_devices = ctx.shared_devices;
    let _shared_messages = ctx.shared_messages;
    let last_exported_devices = ctx.last_exported_devices;
    let selected_channel = ctx.selected_channel;
    match command {
        TuiCommand::Dispatch(DispatchCommand::CreateAccount { name }) => {
            let Some(cb) = callbacks.as_ref() else {
                return Err("App callbacks are unavailable".to_string());
            };
            let Some(update_tx) = update_tx.clone() else {
                return Err("UI update sender is unavailable".to_string());
            };
            let operation = submit_local_terminal_operation(
                app_ctx.app_core.raw().clone(),
                app_ctx.tasks(),
                update_tx,
                OperationId::account_create(),
                SemanticOperationKind::CreateAccount,
            );
            let handle = operation.harness_handle();
            set_authoritative_operation_state_sanctioned(
                state,
                handle.operation_id().clone(),
                Some(handle.instance_id().clone()),
                None,
                OperationState::Submitting,
            );
            (cb.app.on_create_account)(name, operation);
            Ok(Some(handle))
        }
        TuiCommand::Dispatch(DispatchCommand::CreateHome { name, description }) => {
            let Some(cb) = callbacks.as_ref() else {
                return Err("Neighborhood callbacks are unavailable".to_string());
            };
            let Some(update_tx) = update_tx.clone() else {
                return Err("UI update sender is unavailable".to_string());
            };
            let operation = submit_local_terminal_operation(
                app_ctx.app_core.raw().clone(),
                app_ctx.tasks(),
                update_tx,
                OperationId::create_home(),
                SemanticOperationKind::CreateHome,
            );
            let handle = operation.harness_handle();
            (cb.neighborhood.on_create_home)(name, description, operation);
            Ok(Some(handle))
        }
        TuiCommand::Dispatch(DispatchCommand::CreateChannel {
            name,
            topic,
            members,
            threshold_k,
        }) => {
            let Some(cb) = callbacks.as_ref() else {
                return Err("Chat callbacks are unavailable".to_string());
            };
            let Some(update_tx) = update_tx.clone() else {
                return Err("UI update sender is unavailable".to_string());
            };
            state.router.go_to(Screen::Chat);
            let operation = submit_local_terminal_operation(
                app_ctx.app_core.raw().clone(),
                app_ctx.tasks(),
                update_tx,
                OperationId::create_channel(),
                SemanticOperationKind::CreateChannel,
            );
            let handle = operation.harness_handle();
            (cb.chat.on_create_channel)(
                name,
                topic,
                members.into_iter().map(|id| id.to_string()).collect(),
                threshold_k.get(),
                operation,
            );
            Ok(Some(handle))
        }
        TuiCommand::Dispatch(DispatchCommand::SelectChannel { channel_id }) => {
            let channels = shared_channels.read().clone();
            if let Some(idx) = channels.iter().position(|channel| channel.id == channel_id) {
                state.router.go_to(Screen::Chat);
                state.chat.selected_channel = idx;
                *selected_channel.write() =
                    channels.get(idx).map(authoritative_committed_selection);
            } else {
                return Err("Selected channel is no longer visible".to_string());
            }
            Ok(None)
        }
        TuiCommand::Dispatch(DispatchCommand::ImportDeviceEnrollmentDuringOnboarding { code }) => {
            let Some(cb) = callbacks.as_ref() else {
                return Err("App callbacks are unavailable".to_string());
            };
            let Some(update_tx) = update_tx.clone() else {
                return Err("UI update sender is unavailable".to_string());
            };
            let operation = submit_local_terminal_operation(
                app_ctx.app_core.raw().clone(),
                app_ctx.tasks(),
                update_tx,
                OperationId::device_enrollment(),
                SemanticOperationKind::ImportDeviceEnrollmentCode,
            );
            let handle = operation.harness_handle();
            (cb.app.on_import_device_enrollment_during_onboarding)(code, operation);
            Ok(Some(handle))
        }
        TuiCommand::Dispatch(DispatchCommand::CreateInvitation {
            receiver_id,
            invitation_type,
            message,
            ttl_secs,
        }) => {
            let Some(cb) = callbacks.as_ref() else {
                return Err("Invitation callbacks are unavailable".to_string());
            };
            let Some(update_tx) = update_tx.clone() else {
                return Err("UI update sender is unavailable".to_string());
            };
            let kind = match invitation_type {
                InvitationKind::Contact => SemanticOperationKind::CreateContactInvitation,
                InvitationKind::Guardian => SemanticOperationKind::CreateContactInvitation,
                InvitationKind::Channel => SemanticOperationKind::InviteActorToChannel,
            };
            let operation = submit_local_terminal_operation(
                app_ctx.app_core.raw().clone(),
                app_ctx.tasks(),
                update_tx,
                OperationId::invitation_create(),
                kind,
            );
            let handle = operation.harness_handle();
            state.clear_runtime_fact_kind(RuntimeEventKind::InvitationCodeReady);
            (cb.invitations.on_create)(
                receiver_id,
                invitation_type.as_str().to_string(),
                message,
                ttl_secs,
                operation,
            );
            Ok(Some(handle))
        }
        TuiCommand::Dispatch(DispatchCommand::ImportInvitation { code }) => {
            let Some(cb) = callbacks.as_ref() else {
                return Err("Invitation callbacks are unavailable".to_string());
            };
            let Some(update_tx) = update_tx.clone() else {
                return Err("UI update sender is unavailable".to_string());
            };
            let operation = submit_workflow_handoff_operation(
                app_ctx.app_core.raw().clone(),
                app_ctx.tasks(),
                update_tx,
                OperationId::invitation_accept_contact(),
                SemanticOperationKind::AcceptContactInvitation,
            );
            let handle = operation.harness_handle();
            state.clear_runtime_fact_kind(RuntimeEventKind::ContactLinkReady);
            (cb.invitations.on_import)(code, operation);
            Ok(Some(handle))
        }
        TuiCommand::Dispatch(DispatchCommand::AcceptInvitation) => {
            let Some(cb) = callbacks.as_ref() else {
                return Err("Invitation callbacks are unavailable".to_string());
            };
            let Some(update_tx) = update_tx.clone() else {
                return Err("UI update sender is unavailable".to_string());
            };
            let selected = read_selected_notification(
                state.notifications.selected_index,
                shared_invitations,
                shared_pending_requests,
            );
            let Some(NotificationSelection::ReceivedInvitation(invitation_id)) = selected else {
                return Err("Select a received invitation to accept".to_string());
            };
            let accept_kind = semantic_accept_kind_for_invitation(shared_invitations, &invitation_id);
            let operation_id = match accept_kind {
                SemanticOperationKind::AcceptPendingChannelInvitation => OperationId::invitation_accept_channel(),
                _ => OperationId::invitation_accept_contact(),
            };
            let operation = submit_workflow_handoff_operation(
                app_ctx.app_core.raw().clone(),
                app_ctx.tasks(),
                update_tx,
                operation_id,
                accept_kind,
            );
            let handle = operation.harness_handle();
            state.clear_runtime_fact_kind(RuntimeEventKind::ContactLinkReady);
            (cb.invitations.on_accept)(invitation_id, operation);
            Ok(Some(handle))
        }
        TuiCommand::Dispatch(DispatchCommand::JoinChannel { channel_name }) => {
            let Some(cb) = callbacks.as_ref() else {
                return Err("Chat callbacks are unavailable".to_string());
            };
            let Some(update_tx) = update_tx.clone() else {
                return Err("UI update sender is unavailable".to_string());
            };
            let operation = submit_workflow_handoff_operation(
                app_ctx.app_core.raw().clone(),
                app_ctx.tasks(),
                update_tx,
                OperationId::join_channel(),
                SemanticOperationKind::JoinChannel,
            );
            let handle = operation.harness_handle();
            state.router.go_to(Screen::Chat);
            (cb.chat.on_join_channel)(channel_name, operation);
            Ok(Some(handle))
        }
        TuiCommand::Dispatch(DispatchCommand::AcceptPendingChannelInvitation) => {
            let Some(cb) = callbacks.as_ref() else {
                return Err("Chat callbacks are unavailable".to_string());
            };
            let Some(update_tx) = update_tx.clone() else {
                return Err("UI update sender is unavailable".to_string());
            };
            let operation = submit_workflow_handoff_operation(
                app_ctx.app_core.raw().clone(),
                app_ctx.tasks(),
                update_tx,
                OperationId::invitation_accept_channel(),
                SemanticOperationKind::AcceptPendingChannelInvitation,
            );
            let handle = operation.harness_handle();
            state.router.go_to(Screen::Chat);
            state.clear_runtime_fact_kind(RuntimeEventKind::PendingHomeInvitationReady);
            (cb.chat.on_accept_pending_channel_invitation)(operation);
            Ok(Some(handle))
        }
        TuiCommand::Dispatch(DispatchCommand::SendChatMessage { content }) => {
            let Some(cb) = callbacks.as_ref() else {
                return Err("Chat callbacks are unavailable".to_string());
            };
            let trimmed = content.trim_start();
            let operation = if trimmed.starts_with('/') {
                None
            } else {
                let Some(update_tx) = update_tx.clone() else {
                    return Err("UI update sender is unavailable".to_string());
                };
                Some(submit_workflow_handoff_operation(
                    app_ctx.app_core.raw().clone(),
                    app_ctx.tasks(),
                    update_tx,
                    OperationId::send_message(),
                    SemanticOperationKind::SendChatMessage,
                ))
            };
            let channels = shared_channels.read().clone();
            let committed_channel_id = selected_channel
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
                resolve_committed_selected_channel_id(state, &channels)
                    .map(|selection| selection.channel_id().to_string())
            }) {
                let handle = operation
                    .as_ref()
                    .map(WorkflowHandoffOperationOwner::harness_handle);
                if let Some(operation) = operation {
                    (cb.chat.on_send_owned)(channel_id, content, operation);
                } else {
                    (cb.chat.on_run_slash_command)(channel_id, content);
                }
                Ok(handle)
            } else {
                Err(format!(
                    "No committed channel selected (channels={} selected_index={})",
                    channels.len(),
                    state.chat.selected_channel,
                ))
            }
        }
        TuiCommand::Dispatch(DispatchCommand::InviteSelectedContactToChannel) => {
            let Some(cb) = callbacks.as_ref() else {
                return Err("Contact callbacks are unavailable".to_string());
            };
            let contact_idx = state.contacts.selected_index;
            let channel_idx = state.chat.selected_channel;
            let contacts = shared_contacts.read().clone();
            let channels = shared_channels.read().clone();
            let Some(contact) = contacts.get(contact_idx) else {
                return Err("No contact selected".to_string());
            };
            let Some(channel) = channels.get(channel_idx) else {
                return Err("No channel selected".to_string());
            };
            let selected = selected_channel.read().clone();
            let context_id =
                strongest_authoritative_binding_for_channel(channel, selected.as_ref())
                    .and_then(|binding| binding.context_id);
            let Some(context_id) = context_id else {
                return Err(format!(
                    "Selected channel lacks authoritative context: {}",
                    channel.id
                ));
            };
            let Some(update_tx) = update_tx.clone() else {
                return Err("UI update sender is unavailable".to_string());
            };
            let operation = submit_workflow_handoff_operation(
                app_ctx.app_core.raw().clone(),
                app_ctx.tasks(),
                update_tx,
                OperationId::invitation_create(),
                SemanticOperationKind::InviteActorToChannel,
            );
            let handle = operation.harness_handle();
            state.clear_runtime_fact_kind(RuntimeEventKind::PendingHomeInvitationReady);
            (cb.contacts.on_invite_to_channel)(
                contact.id.clone(),
                channel.id.clone(),
                Some(context_id),
                operation,
            );
            Ok(Some(handle))
        }
        TuiCommand::Dispatch(DispatchCommand::InviteActorToChannel {
            authority_id,
            channel_id,
        }) => {
            let Some(cb) = callbacks.as_ref() else {
                return Err("Contact callbacks are unavailable".to_string());
            };
            let channels = shared_channels.read().clone();
            let channel_id_string = channel_id.clone();
            let Some(channel) = channels
                .iter()
                .find(|channel| channel.id == channel_id_string)
            else {
                return Err(format!(
                    "Selected channel is stale or unavailable: {channel_id}"
                ));
            };
            let selected = selected_channel.read().clone();
            let context_id =
                strongest_authoritative_binding_for_channel(channel, selected.as_ref())
                    .and_then(|binding| binding.context_id);
            let Some(context_id) = context_id else {
                return Err(format!(
                    "Selected channel lacks authoritative context: {}",
                    channel.id
                ));
            };
            let Some(update_tx) = update_tx.clone() else {
                return Err("UI update sender is unavailable".to_string());
            };
            let operation = submit_workflow_handoff_operation(
                app_ctx.app_core.raw().clone(),
                app_ctx.tasks(),
                update_tx,
                OperationId::invitation_create(),
                SemanticOperationKind::InviteActorToChannel,
            );
            let handle = operation.harness_handle();
            state.clear_runtime_fact_kind(RuntimeEventKind::PendingHomeInvitationReady);
            (cb.contacts.on_invite_to_channel)(
                authority_id.to_string(),
                channel.id.clone(),
                Some(context_id),
                operation,
            );
            Ok(Some(handle))
        }
        TuiCommand::Dispatch(DispatchCommand::AddDevice {
            name,
            invitee_authority_id,
        }) => {
            let Some(cb) = callbacks.as_ref() else {
                return Err("Settings callbacks are unavailable".to_string());
            };
            let Some(update_tx) = update_tx.clone() else {
                return Err("UI update sender is unavailable".to_string());
            };
            let operation = submit_local_terminal_operation(
                app_ctx.app_core.raw().clone(),
                app_ctx.tasks(),
                update_tx,
                OperationId::device_enrollment(),
                SemanticOperationKind::StartDeviceEnrollment,
            );
            let handle = operation.harness_handle();
            (cb.settings.on_add_device)(name, invitee_authority_id, operation);
            Ok(Some(handle))
        }
        TuiCommand::Dispatch(DispatchCommand::RemoveDevice { device_id }) => {
            let Some(cb) = callbacks.as_ref() else {
                return Err("Settings callbacks are unavailable".to_string());
            };
            let Some(update_tx) = update_tx.clone() else {
                return Err("UI update sender is unavailable".to_string());
            };
            let operation = submit_ceremony_operation(
                app_ctx.app_core.raw().clone(),
                app_ctx.tasks(),
                update_tx,
                OperationId::remove_device(),
                SemanticOperationKind::RemoveDevice,
            );
            let handle = operation.harness_handle();
            (cb.settings.on_remove_device)(device_id, operation);
            Ok(Some(handle))
        }
        TuiCommand::Dispatch(DispatchCommand::OpenDeviceSelectModal) => {
            let current_devices = shared_devices.read().clone();

            if current_devices.is_empty() {
                return Err("No devices to remove".to_string());
            }

            let has_removable = current_devices.iter().any(|device| !device.is_current);
            if !has_removable {
                return Err("Cannot remove the current device".to_string());
            }

            let devices = current_devices
                .iter()
                .map(|device| Device {
                    id: device.id.clone(),
                    name: if device.name.is_empty() {
                        let short = device.id.chars().take(8).collect::<String>();
                        format!("Device {short}")
                    } else {
                        device.name.clone()
                    },
                    is_current: device.is_current,
                    last_seen: device.last_seen,
                })
                .collect::<Vec<_>>();

            let modal_state = crate::tui::state::DeviceSelectModalState::with_devices(devices);
            state
                .modal_queue
                .enqueue(crate::tui::state::QueuedModal::SettingsDeviceSelect(
                    modal_state,
                ));
            Ok(None)
        }
        TuiCommand::HarnessRemoveVisibleDevice { device_id } => {
            let current_devices = shared_devices.read().clone();
            let exported_devices = last_exported_devices.read().clone();
            let Some(device_id) = device_id
                .or_else(|| {
                    current_devices
                        .iter()
                        .find(|device| !device.is_current)
                        .map(|device| device.id.clone())
                })
                .or_else(|| {
                    (current_devices.len() > 1)
                        .then(|| current_devices.last().map(|device| device.id.clone()))
                        .flatten()
                })
                .or_else(|| {
                    exported_devices
                        .iter()
                        .find(|device| !device.is_current)
                        .map(|device| device.id.clone())
                        .or_else(|| {
                            (exported_devices.len() > 1)
                                .then(|| exported_devices.last().map(|device| device.id.clone()))
                                .flatten()
                        })
                })
            else {
                return Err(format!(
                    "no removable device is visible (shared_devices={} exported_devices={})",
                    current_devices
                        .iter()
                        .map(|device| format!("{}:current={}", device.id, device.is_current))
                        .collect::<Vec<_>>()
                        .join(","),
                    exported_devices
                        .iter()
                        .map(|device| format!("{}:current={}", device.id, device.is_current))
                        .collect::<Vec<_>>()
                        .join(",")
                ));
            };
            let Some(cb) = callbacks.as_ref() else {
                return Err("Settings callbacks are unavailable".to_string());
            };
            let Some(update_tx) = update_tx.clone() else {
                return Err("UI update sender is unavailable".to_string());
            };
            let operation = submit_ceremony_operation(
                app_ctx.app_core.raw().clone(),
                app_ctx.tasks(),
                update_tx,
                OperationId::remove_device(),
                SemanticOperationKind::RemoveDevice,
            );
            let handle = operation.harness_handle();
            (cb.settings.on_remove_device)(device_id.into(), operation);
            Ok(Some(handle))
        }
        _ => Ok(None),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EventCommandLoopAction {
    ContinueCommand,
    Handled,
}

pub(super) struct EventDispatchContext<'a> {
    pub app_ctx: &'a AppCoreContext,
    pub callbacks: &'a CallbackRegistry,
    pub tasks_for_events: &'a Arc<UiTaskOwner>,
    pub update_tx_for_events: &'a Option<UiUpdateSender>,
    pub update_tx_for_dispatch: &'a Option<UiUpdateSender>,
    pub update_tx_for_ceremony: &'a Option<UiUpdateSender>,
    pub shared_channels_for_dispatch: &'a SharedChannels,
    pub shared_neighborhood_homes_for_dispatch: &'a Arc<parking_lot::RwLock<Vec<String>>>,
    pub shared_invitations_for_dispatch: &'a SharedInvitations,
    pub shared_pending_requests_for_dispatch: &'a SharedPendingRequests,
    pub shared_contacts_for_dispatch: &'a SharedContacts,
    pub shared_discovered_peers_for_dispatch: &'a SharedDiscoveredPeers,
    pub shared_messages_for_dispatch: &'a SharedMessages,
    pub shared_devices_for_dispatch: &'a SharedDevices,
    pub shared_threshold_for_dispatch: &'a SharedThreshold,
    pub tui_selected_for_events: &'a SharedCommittedChannelSelection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CeremonySetupOwnerClass {
    ExplicitGuardianSelection,
    ExplicitDeviceSelection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModalOpenContract {
    AuthoritativeSelection,
    ObservedOnlyConvenience,
    CeremonyBootstrap(CeremonySetupOwnerClass),
}

fn open_chat_modal_from_authoritative_selection(
    dispatch_cmd: &DispatchCommand,
    new_state: &mut TuiState,
    shared_channels_for_dispatch: &SharedChannels,
    cb: &CallbackRegistry,
) -> Option<EventCommandLoopAction> {
    let contract = ModalOpenContract::AuthoritativeSelection;
    let idx = new_state.chat.selected_channel;
    let channels = shared_channels_for_dispatch.read().clone();
    match dispatch_cmd {
        DispatchCommand::OpenChatTopicModal => {
            debug_assert!(matches!(
                contract,
                ModalOpenContract::AuthoritativeSelection
            ));
            if let Some(channel) = channels.get(idx) {
                let modal_state = crate::tui::state::TopicModalState::for_channel(
                    &channel.id,
                    channel.topic.as_deref().unwrap_or(""),
                );
                new_state
                    .modal_queue
                    .enqueue(crate::tui::state::QueuedModal::ChatTopic(modal_state));
            } else {
                new_state.toast_error("No channel selected");
            }
            Some(EventCommandLoopAction::Handled)
        }
        DispatchCommand::OpenChatInfoModal => {
            debug_assert!(matches!(
                contract,
                ModalOpenContract::AuthoritativeSelection
            ));
            if let Some(channel) = channels.get(idx) {
                let mut modal_state = crate::tui::state::ChannelInfoModalState::for_channel(
                    &channel.id,
                    &channel.name,
                    channel.topic.as_deref(),
                );

                let mut participants = vec!["You".to_string()];
                if participants.len() <= 1 && channel.member_count > 1 {
                    let extra = channel
                        .member_count
                        .saturating_sub(participants.len() as u32);
                    if extra > 0 {
                        participants.push(format!("+{extra} others"));
                    }
                }

                modal_state.participants = participants;
                new_state
                    .modal_queue
                    .enqueue(crate::tui::state::QueuedModal::ChatInfo(modal_state));
                (cb.chat.on_list_participants)(channel.id.clone());
            } else {
                new_state.toast_error("No channel selected");
            }
            Some(EventCommandLoopAction::Handled)
        }
        _ => None,
    }
}

fn open_ceremony_setup_modal(
    dispatch_cmd: &DispatchCommand,
    new_state: &mut TuiState,
    shared_contacts_for_dispatch: &SharedContacts,
    shared_devices_for_dispatch: &SharedDevices,
) -> Option<EventCommandLoopAction> {
    match dispatch_cmd {
        DispatchCommand::OpenGuardianSetup => {
            debug_assert!(matches!(
                ModalOpenContract::CeremonyBootstrap(
                    CeremonySetupOwnerClass::ExplicitGuardianSelection
                ),
                ModalOpenContract::CeremonyBootstrap(_)
            ));
            let current_contacts = shared_contacts_for_dispatch.read().clone();
            if current_contacts.is_empty() {
                new_state.toast_error(GuardianSetupError::NoContacts.to_string());
                return Some(EventCommandLoopAction::ContinueCommand);
            }

            let candidates: Vec<crate::tui::state::GuardianCandidate> = current_contacts
                .iter()
                .map(|c| crate::tui::state::GuardianCandidate {
                    id: c.id.clone(),
                    name: c.nickname.clone(),
                    is_current_guardian: c.is_guardian,
                })
                .collect();
            let selected: Vec<usize> = candidates
                .iter()
                .enumerate()
                .filter(|(_, c)| c.is_current_guardian)
                .map(|(i, _)| i)
                .collect();

            let modal_state =
                crate::tui::state::GuardianSetupModalState::from_contacts_with_selection(
                    candidates, selected,
                );
            new_state
                .modal_queue
                .enqueue(crate::tui::state::QueuedModal::GuardianSetup(modal_state));
            Some(EventCommandLoopAction::Handled)
        }
        DispatchCommand::OpenMfaSetup => {
            debug_assert!(matches!(
                ModalOpenContract::CeremonyBootstrap(
                    CeremonySetupOwnerClass::ExplicitDeviceSelection
                ),
                ModalOpenContract::CeremonyBootstrap(_)
            ));
            let current_devices = shared_devices_for_dispatch.read().clone();
            if current_devices.len() < MIN_MFA_DEVICES {
                new_state.toast_error(
                    MfaSetupError::InsufficientDevices {
                        required: MIN_MFA_DEVICES,
                        available: current_devices.len(),
                    }
                    .to_string(),
                );
                return Some(EventCommandLoopAction::ContinueCommand);
            }

            let candidates: Vec<crate::tui::state::GuardianCandidate> = current_devices
                .iter()
                .map(|d| {
                    let name = if d.name.is_empty() {
                        let short = d.id.chars().take(8).collect::<String>();
                        format!("Device {short}")
                    } else {
                        d.name.clone()
                    };
                    crate::tui::state::GuardianCandidate {
                        id: d.id.clone(),
                        name,
                        is_current_guardian: d.is_current,
                    }
                })
                .collect();
            let n = candidates.len() as u8;
            let threshold_k = aura_app::ui::types::default_guardian_threshold(n);
            let modal_state =
                crate::tui::state::GuardianSetupModalState::for_mfa_setup(candidates, threshold_k);
            new_state
                .modal_queue
                .enqueue(crate::tui::state::QueuedModal::MfaSetup(modal_state));
            Some(EventCommandLoopAction::Handled)
        }
        _ => None,
    }
}

fn open_observed_convenience_modal(
    dispatch_cmd: &DispatchCommand,
    new_state: &mut TuiState,
    shared_contacts_for_dispatch: &SharedContacts,
    shared_devices_for_dispatch: &SharedDevices,
) -> Option<EventCommandLoopAction> {
    debug_assert!(matches!(
        ModalOpenContract::ObservedOnlyConvenience,
        ModalOpenContract::ObservedOnlyConvenience
    ));
    match dispatch_cmd {
        DispatchCommand::OpenCreateInvitationModal => {
            let idx = new_state.contacts.selected_index;
            let guard = shared_contacts_for_dispatch.read();
            let mut modal_state = if let Some(contact) = guard.get(idx) {
                crate::tui::state::CreateInvitationModalState::for_receiver(
                    contact.id.clone(),
                    contact.nickname.clone(),
                )
            } else {
                crate::tui::state::CreateInvitationModalState::new()
            };
            modal_state.type_index = 1;
            new_state
                .modal_queue
                .enqueue(crate::tui::state::QueuedModal::ContactsCreate(modal_state));
            Some(EventCommandLoopAction::Handled)
        }
        DispatchCommand::OpenDeviceSelectModal => {
            let current_devices = shared_devices_for_dispatch.read().clone();
            if current_devices.is_empty() {
                new_state.toast_info("No devices to remove");
                return Some(EventCommandLoopAction::ContinueCommand);
            }
            if !current_devices.iter().any(|d| !d.is_current) {
                new_state.toast_info("Cannot remove the current device");
                return Some(EventCommandLoopAction::ContinueCommand);
            }
            let devices: Vec<crate::tui::types::Device> = current_devices
                .iter()
                .map(|d| crate::tui::types::Device {
                    id: d.id.clone(),
                    name: if d.name.is_empty() {
                        let short = d.id.chars().take(8).collect::<String>();
                        format!("Device {short}")
                    } else {
                        d.name.clone()
                    },
                    is_current: d.is_current,
                    last_seen: d.last_seen,
                })
                .collect();
            let modal_state = crate::tui::state::DeviceSelectModalState::with_devices(devices);
            new_state
                .modal_queue
                .enqueue(crate::tui::state::QueuedModal::SettingsDeviceSelect(
                    modal_state,
                ));
            Some(EventCommandLoopAction::Handled)
        }
        _ => None,
    }
}

pub(super) fn handle_dispatch_command(
    dispatch_cmd: DispatchCommand,
    new_state: &mut TuiState,
    event_ctx: &EventDispatchContext<'_>,
) -> EventCommandLoopAction {
    if let Some(result) = open_chat_modal_from_authoritative_selection(
        &dispatch_cmd,
        new_state,
        event_ctx.shared_channels_for_dispatch,
        event_ctx.callbacks,
    ) {
        return result;
    }
    if let Some(result) = open_ceremony_setup_modal(
        &dispatch_cmd,
        new_state,
        event_ctx.shared_contacts_for_dispatch,
        event_ctx.shared_devices_for_dispatch,
    ) {
        return result;
    }
    if let Some(result) = open_observed_convenience_modal(
        &dispatch_cmd,
        new_state,
        event_ctx.shared_contacts_for_dispatch,
        event_ctx.shared_devices_for_dispatch,
    ) {
        return result;
    }

    handle_dispatch_command_match(dispatch_cmd, new_state, event_ctx)
}

#[cfg(test)]
mod tests {
    use super::authoritative_binding_for_requested_join;
    use crate::tui::types::Channel;
    use aura_app::scenario_contract::SemanticCommandValue;
    use aura_app::ui::contract::HarnessUiCommand;
    use aura_app::ui_contract::ChannelBindingWitness;

    #[test]
    fn immediate_join_binding_requires_authoritative_context() {
        let command = HarnessUiCommand::JoinChannel {
            channel_name: "shared-parity-lab".to_string(),
        };
        let channels = vec![Channel::new("channel-1", "shared-parity-lab")];
        let weak_binding = ChannelBindingWitness::new("channel-1", None);

        let value = authoritative_binding_for_requested_join(
            &command,
            &channels,
            Some(0),
            Some(&weak_binding),
        );

        assert!(value.is_none());
    }

    #[test]
    fn immediate_join_binding_accepts_authoritative_selected_binding() {
        let command = HarnessUiCommand::JoinChannel {
            channel_name: "#shared-parity-lab".to_string(),
        };
        let mut channel = Channel::new("channel-1", "shared-parity-lab");
        channel.context_id = Some("ctx-1".parse().expect("valid context id"));
        let channels = vec![channel];
        let binding = ChannelBindingWitness::new("channel-1", Some("ctx-1".to_string()));

        let value =
            authoritative_binding_for_requested_join(&command, &channels, Some(0), Some(&binding));

        assert!(matches!(
            value,
            Some(SemanticCommandValue::AuthoritativeChannelBinding {
                ref channel_id,
                ref context_id
            }) if channel_id == "channel-1" && context_id == "ctx-1"
        ));
    }
}
