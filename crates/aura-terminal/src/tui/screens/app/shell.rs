//! # App Shell
//!
//! Main application shell with screen navigation and modal management.
//!
//! This is the root TUI component that coordinates all screens, handles
//! events, manages the state machine, and renders modals.

// Allow field reassignment for large structs with many conditional fields
#![allow(clippy::field_reassign_with_default)]
// Allow manual map patterns in element! macro contexts for clarity
#![allow(clippy::manual_map)]

use super::modal_overlays::{
    render_access_override_modal, render_account_setup_modal, render_add_device_modal,
    render_capability_config_modal, render_channel_info_modal, render_chat_create_modal,
    render_confirm_modal, render_contact_modal, render_contacts_code_modal,
    render_contacts_create_modal, render_contacts_import_modal, render_device_enrollment_modal,
    render_device_import_modal, render_device_select_modal, render_guardian_modal,
    render_guardian_setup_modal, render_help_modal, render_home_create_modal,
    render_mfa_setup_modal, render_moderator_assignment_modal, render_nickname_modal,
    render_nickname_suggestion_modal, render_remove_device_modal, render_topic_modal,
    GlobalModalProps,
};

use crate::tui::components::copy_to_clipboard;
use iocraft::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use aura_app::ceremonies::{
    ChannelError, GuardianSetupError, MfaSetupError, RecoveryError, MIN_CHANNEL_PARTICIPANTS,
    MIN_MFA_DEVICES,
};
use aura_app::harness_mode_enabled;
use aura_app::ui::contract::OperationState;
use aura_app::ui::prelude::*;
use aura_app::ui::signals::{NetworkStatus, ERROR_SIGNAL, SETTINGS_SIGNAL};
use aura_app::ui::workflows::access as access_workflows;
use aura_app::ui::workflows::ceremonies::{
    monitor_key_rotation_ceremony, start_device_threshold_ceremony, start_guardian_ceremony,
};
use aura_app::ui::workflows::network as network_workflows;
use aura_app::ui::workflows::settings::refresh_settings_from_runtime;
use aura_app::ui::workflows::system as system_workflows;
use aura_app::ui_contract::{
    HarnessUiCommand, RuntimeEventKind, RuntimeFact, SemanticOperationKind,
};
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::types::FrostThreshold;
use aura_core::{
    execute_with_retry_budget, ExponentialBackoffPolicy, RetryBudgetPolicy, RetryRunError,
    TimeoutExecutionProfile,
};
use aura_effects::time::PhysicalTimeHandler;

use crate::error::TerminalError;
use crate::tui::callbacks::CallbackRegistry;
use crate::tui::components::{
    DiscoveredPeerInfo, Footer, NavBar, ToastContainer, ToastLevel, ToastMessage,
};
use crate::tui::context::IoContext;
use crate::tui::harness_state::{
    apply_harness_command, clear_harness_command_sender, ensure_harness_command_listener,
    maybe_export_ui_snapshot, register_harness_command_sender, TuiSemanticInputs,
};
use crate::tui::hooks::{AppCoreContext, CallbackContext};
use crate::tui::keymap::{global_footer_hints, screen_footer_hints};
use crate::tui::layout::dim;
use crate::tui::navigation::clamp_list_index;
use crate::tui::screens::app::subscriptions::{
    use_authoritative_semantic_facts_subscription, use_authority_id_subscription,
    use_channels_subscription, use_contacts_subscription, use_devices_subscription,
    use_discovered_peers_subscription, use_invitations_subscription, use_messages_subscription,
    use_nav_status_signals, use_neighborhood_home_meta_subscription,
    use_neighborhood_homes_subscription, use_notifications_subscription,
    use_pending_requests_subscription, use_threshold_subscription, SharedChannels, SharedContacts,
    SharedDevices, SharedMessages, SharedNeighborhoodHomeMeta,
};

async fn effect_sleep(duration: std::time::Duration) {
    let _ = PhysicalTimeHandler::new()
        .sleep_ms(duration.as_millis() as u64)
        .await;
}

#[allow(clippy::expect_used)]
fn shell_retry_policy() -> RetryBudgetPolicy {
    let profile = if harness_mode_enabled() {
        TimeoutExecutionProfile::harness()
    } else {
        TimeoutExecutionProfile::production()
    };
    let base = RetryBudgetPolicy::new(
        200,
        ExponentialBackoffPolicy::new(
            std::time::Duration::from_millis(50),
            std::time::Duration::from_secs(2),
            profile.jitter(),
        )
        .expect("shell backoff policy must be valid"),
    );
    profile
        .apply_retry_policy(&base)
        .expect("shell retry policy must scale")
}

async fn authoritative_settings_devices_for_command(
    app_ctx: &AppCoreContext,
    shared_devices: &SharedDevices,
) -> Vec<Device> {
    let shared = shared_devices.read().clone();
    let mut from_signal = {
        let core = app_ctx.app_core.raw().read().await;
        core.reactive().read(&*SETTINGS_SIGNAL).await.ok()
    };

    if from_signal
        .as_ref()
        .is_none_or(|settings_state| settings_state.devices.is_empty())
    {
        let _ = refresh_settings_from_runtime(app_ctx.app_core.raw()).await;
        from_signal = {
            let core = app_ctx.app_core.raw().read().await;
            core.reactive().read(&*SETTINGS_SIGNAL).await.ok()
        };
    }

    if let Some(settings_state) = from_signal {
        let devices = settings_state
            .devices
            .iter()
            .map(|device| Device {
                id: device.id.to_string(),
                name: device.name.clone(),
                is_current: device.is_current,
                last_seen: device.last_seen,
            })
            .collect::<Vec<_>>();
        if !devices.is_empty() {
            *shared_devices.write() = devices.clone();
        }
        return devices;
    }

    shared
}

async fn authoritative_settings_authorities_for_command(
    app_ctx: &AppCoreContext,
) -> (Vec<crate::tui::types::AuthorityInfo>, usize) {
    let from_signal = {
        let core = app_ctx.app_core.raw().read().await;
        core.reactive().read(&*SETTINGS_SIGNAL).await.ok()
    };

    if let Some(settings_state) = from_signal {
        let current_index = settings_state
            .authorities
            .iter()
            .position(|authority| authority.is_current)
            .unwrap_or(0);
        let authorities = settings_state
            .authorities
            .iter()
            .map(|authority| {
                let info = crate::tui::types::AuthorityInfo::new(
                    authority.id.to_string(),
                    authority.nickname_suggestion.clone(),
                );
                if authority.is_current {
                    info.current()
                } else {
                    info
                }
            })
            .collect::<Vec<_>>();
        return (authorities, current_index);
    }

    (Vec::new(), 0)
}
use crate::tui::screens::router::Screen;
use crate::tui::screens::{
    ChatScreen, ContactsScreen, NeighborhoodScreen, NotificationsScreen, SettingsScreen,
};
use crate::tui::state::InvitationKind;
use crate::tui::types::{
    AccessLevel, Channel, Contact, Device, Guardian, HomeSummary, Invitation, Message, MfaPolicy,
};

// State machine integration
use crate::tui::iocraft_adapter::convert_iocraft_event;
use crate::tui::props::{
    extract_chat_view_props, extract_contacts_view_props, extract_neighborhood_view_props,
    extract_notifications_view_props, extract_settings_view_props,
};
use crate::tui::semantic_lifecycle::{LocalTerminalOperationOwner, WorkflowHandoffOperationOwner};
use crate::tui::state::{transition, DispatchCommand, QueuedModal, TuiCommand, TuiState};
use crate::tui::updates::{
    harness_command_channel, ui_update_channel, HarnessCommandReceiptHandle,
    HarnessCommandReceiver, UiOperation, UiOperationFailure, UiUpdate, UiUpdateReceiver,
    UiUpdateSender,
};
use std::sync::Mutex;

mod events;
mod input;
mod render;
mod state;
use events::{handle_channel_selection_change, resolve_committed_selected_channel_id};
use input::transition_from_terminal_event;
use render::{build_global_modals, state_indicator_label};
use state::{sync_neighborhood_navigation_state, TuiStateHandle};

#[derive(Clone, Debug, PartialEq, Eq)]
enum NotificationSelection {
    ReceivedInvitation(String),
    SentInvitation(String),
    RecoveryRequest(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SelectedChannelBinding {
    channel_id: String,
    context_id: Option<String>,
}

impl SelectedChannelBinding {
    fn merged_from_channel(channel: &Channel, previous: Option<&Self>) -> Self {
        let preserved_context = previous
            .filter(|binding| binding.channel_id == channel.id)
            .and_then(|binding| binding.context_id.clone());
        Self {
            channel_id: channel.id.clone(),
            context_id: channel.context_id.clone().or(preserved_context),
        }
    }
}

fn complete_ready_channel_binding_receipts(
    pending_receipts: &Arc<Mutex<HashMap<String, HarnessCommandReceiptHandle>>>,
    ready_receipts: &Arc<Mutex<HashSet<String>>>,
    operation_id: aura_app::ui_contract::OperationId,
    binding: &SelectedChannelBinding,
) {
    let ready_instance_ids = {
        let mut ready = ready_receipts.lock().unwrap();
        ready.drain().collect::<Vec<_>>()
    };
    if ready_instance_ids.is_empty() {
        return;
    }
    let mut pending = pending_receipts.lock().unwrap();
    for instance_id in ready_instance_ids {
        let Some(receipt) = pending.remove(&instance_id) else {
            continue;
        };
        receipt.complete(
            aura_app::ui::contract::HarnessUiCommandReceipt::AcceptedWithOperation {
                operation: aura_app::ui_contract::HarnessUiOperationHandle::new(
                    operation_id.clone(),
                    aura_app::ui_contract::OperationInstanceId(instance_id),
                ),
                value: Some(
                    aura_app::scenario_contract::SemanticCommandValue::ChannelBinding {
                        channel_id: binding.channel_id.clone(),
                        context_id: binding.context_id.clone(),
                    },
                ),
            },
        );
    }
}

fn terminal_error_to_toast_level(error: &TerminalError) -> crate::tui::state::ToastLevel {
    match error.category().toast_severity() {
        aura_app::errors::ToastLevel::Info => crate::tui::state::ToastLevel::Info,
        aura_app::errors::ToastLevel::Success => crate::tui::state::ToastLevel::Success,
        aura_app::errors::ToastLevel::Warning => crate::tui::state::ToastLevel::Warning,
        aura_app::errors::ToastLevel::Error => crate::tui::state::ToastLevel::Error,
    }
}

fn format_ui_operation_failure(failure: &UiOperationFailure) -> String {
    let category = failure.error.category();
    format!(
        "[{}] {}: {}. Hint: {}",
        failure.error.code(),
        failure.operation.label(),
        failure.error.message(),
        category.resolution_hint(),
    )
}

async fn send_optional_ui_update_required(tx: &Option<UiUpdateSender>, update: UiUpdate) {
    if let Some(tx) = tx {
        if tx.try_send(update.clone()).is_err() {
            let _ = tx.send(update).await;
        }
    }
}

fn read_selected_notification(
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

fn execute_harness_followup_command(
    state: &mut TuiState,
    command: TuiCommand,
    callbacks: &Option<CallbackRegistry>,
    app_ctx: &AppCoreContext,
    update_tx: &Option<UiUpdateSender>,
    shared_contacts: &SharedContacts,
    shared_channels: &SharedChannels,
    shared_devices: &SharedDevices,
    shared_messages: &SharedMessages,
    last_exported_devices: &std::sync::Arc<parking_lot::RwLock<Vec<Device>>>,
    selected_channel: &std::sync::Arc<parking_lot::RwLock<Option<String>>>,
    selected_channel_binding: &std::sync::Arc<parking_lot::RwLock<Option<SelectedChannelBinding>>>,
) -> Result<Option<aura_app::ui_contract::HarnessUiOperationHandle>, String> {
    match command {
        TuiCommand::Dispatch(DispatchCommand::CreateAccount { name }) => {
            let Some(cb) = callbacks.as_ref() else {
                return Err("App callbacks are unavailable".to_string());
            };
            let Some(update_tx) = update_tx.clone() else {
                return Err("UI update sender is unavailable".to_string());
            };
            let operation = LocalTerminalOperationOwner::submit(
                app_ctx.app_core.raw().clone(),
                app_ctx.tasks(),
                update_tx,
                OperationId::account_create(),
                SemanticOperationKind::CreateAccount,
            );
            let handle = operation.harness_handle();
            (cb.app.on_create_account)(name, operation);
            Ok(Some(handle))
        }
        TuiCommand::Dispatch(DispatchCommand::CreateHome { name, description }) => {
            let Some(cb) = callbacks.as_ref() else {
                return Err("Neighborhood callbacks are unavailable".to_string());
            };
            (cb.neighborhood.on_create_home)(name, description);
            Ok(None)
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
            let operation = LocalTerminalOperationOwner::submit(
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
                Some(operation),
            );
            Ok(Some(handle))
        }
        TuiCommand::Dispatch(DispatchCommand::SelectChannel { channel_id }) => {
            let channels = shared_channels.read().clone();
            if let Some(idx) = channels.iter().position(|channel| channel.id == channel_id) {
                state.router.go_to(Screen::Chat);
                state.chat.selected_channel = idx;
                *selected_channel.write() = Some(channel_id.to_string());
                {
                    let mut guard = selected_channel_binding.write();
                    let previous = guard.clone();
                    *guard = channels.get(idx).map(|channel| {
                        SelectedChannelBinding::merged_from_channel(channel, previous.as_ref())
                    });
                }
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
            let operation = LocalTerminalOperationOwner::submit(
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
            let operation = LocalTerminalOperationOwner::submit(
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
                Some(operation),
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
            let operation = WorkflowHandoffOperationOwner::submit(
                app_ctx.app_core.raw().clone(),
                app_ctx.tasks(),
                update_tx,
                OperationId::invitation_accept(),
                SemanticOperationKind::AcceptContactInvitation,
            );
            let handle = operation.harness_handle();
            state.clear_runtime_fact_kind(RuntimeEventKind::ContactLinkReady);
            (cb.invitations.on_import)(code, operation);
            Ok(Some(handle))
        }
        TuiCommand::Dispatch(DispatchCommand::JoinChannel { channel_name }) => {
            let Some(cb) = callbacks.as_ref() else {
                return Err("Chat callbacks are unavailable".to_string());
            };
            let Some(update_tx) = update_tx.clone() else {
                return Err("UI update sender is unavailable".to_string());
            };
            let operation = WorkflowHandoffOperationOwner::submit(
                app_ctx.app_core.raw().clone(),
                app_ctx.tasks(),
                update_tx,
                OperationId::join_channel(),
                SemanticOperationKind::JoinChannel,
            );
            let handle = operation.harness_handle();
            state.router.go_to(Screen::Chat);
            (cb.chat.on_join_channel)(channel_name, Some(operation));
            Ok(Some(handle))
        }
        TuiCommand::Dispatch(DispatchCommand::AcceptPendingHomeInvitation) => {
            let Some(cb) = callbacks.as_ref() else {
                return Err("Chat callbacks are unavailable".to_string());
            };
            let Some(update_tx) = update_tx.clone() else {
                return Err("UI update sender is unavailable".to_string());
            };
            let operation = WorkflowHandoffOperationOwner::submit(
                app_ctx.app_core.raw().clone(),
                app_ctx.tasks(),
                update_tx,
                OperationId::invitation_accept(),
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
                Some(WorkflowHandoffOperationOwner::submit(
                    app_ctx.app_core.raw().clone(),
                    app_ctx.tasks(),
                    update_tx,
                    OperationId::send_message(),
                    SemanticOperationKind::SendChatMessage,
                ))
            };
            let channels = shared_channels.read().clone();
            let committed_channel_id = selected_channel.read().clone().filter(|channel_id| {
                channels.is_empty() || channels.iter().any(|channel| channel.id == *channel_id)
            });
            let visible_message_channel_id = shared_messages
                .read()
                .last()
                .map(|message| message.channel_id.clone())
                .filter(|channel_id| {
                    channels.is_empty() || channels.iter().any(|channel| channel.id == *channel_id)
                });
            if let Some(channel_id) = committed_channel_id
                .or_else(|| resolve_committed_selected_channel_id(state, &channels))
                .or(visible_message_channel_id)
            {
                let handle = operation
                    .as_ref()
                    .map(WorkflowHandoffOperationOwner::harness_handle);
                (cb.chat.on_send)(channel_id, content, operation);
                Ok(handle)
            } else {
                Err(format!(
                    "No committed channel selected (channels={} selected_index={} visible_messages={})",
                    channels.len(),
                    state.chat.selected_channel,
                    shared_messages.read().len()
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
            let selected_binding = selected_channel_binding
                .read()
                .clone()
                .filter(|binding| binding.channel_id == channel.id);
            let context_id = selected_binding
                .and_then(|binding| binding.context_id)
                .or_else(|| channel.context_id.clone());
            let Some(context_id) = context_id else {
                return Err(format!(
                    "Selected channel lacks authoritative context: {}",
                    channel.id
                ));
            };
            state.clear_runtime_fact_kind(RuntimeEventKind::PendingHomeInvitationReady);
            (cb.contacts.on_invite_to_channel)(
                contact.id.clone(),
                channel.id.clone(),
                Some(context_id),
                None,
            );
            Ok(None)
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
            let selected_binding = selected_channel_binding
                .read()
                .clone()
                .filter(|binding| binding.channel_id == channel.id);
            let context_id = selected_binding
                .and_then(|binding| binding.context_id)
                .or_else(|| channel.context_id.clone());
            let Some(context_id) = context_id else {
                return Err(format!(
                    "Selected channel lacks authoritative context: {}",
                    channel.id
                ));
            };
            let Some(update_tx) = update_tx.clone() else {
                return Err("UI update sender is unavailable".to_string());
            };
            let operation = WorkflowHandoffOperationOwner::submit(
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
                Some(operation),
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
            let operation = LocalTerminalOperationOwner::submit(
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
            (cb.settings.on_remove_device)(device_id);
            Ok(None)
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
            (cb.settings.on_remove_device)(device_id.into());
            Ok(None)
        }
        _ => Ok(None),
    }
}

/// Props for IoApp
///
/// These values are initial seeds only. Screens subscribe to `aura_app` signals
/// for live data and will overwrite these props immediately on mount.
#[derive(Default, Props)]
pub struct IoAppProps {
    // Screen data - initial seeds only (live data comes from signal subscriptions)
    pub channels: Vec<Channel>,
    pub messages: Vec<Message>,
    pub invitations: Vec<Invitation>,
    pub guardians: Vec<Guardian>,
    pub devices: Vec<Device>,
    pub nickname_suggestion: String,
    pub threshold_k: u8,
    pub threshold_n: u8,
    pub mfa_policy: MfaPolicy,
    // Contacts screen data
    pub contacts: Vec<Contact>,
    /// Discovered LAN peers
    pub discovered_peers: Vec<DiscoveredPeerInfo>,
    // Neighborhood screen data
    pub neighborhood_name: String,
    pub homes: Vec<HomeSummary>,
    pub access_level: AccessLevel,
    // Account setup
    /// Whether to show account setup modal on start
    pub show_account_setup: bool,
    /// Whether startup runtime bootstrap is still converging.
    pub pending_runtime_bootstrap: bool,
    // Network status
    /// Unified network status (disconnected, no peers, syncing, synced)
    pub network_status: NetworkStatus,
    /// Transport-level peers (active network connections)
    pub transport_peers: usize,
    /// Online contacts (people you know who are currently online)
    pub known_online: usize,
    // Demo mode
    /// Whether running in demo mode
    #[cfg(feature = "development")]
    pub demo_mode: bool,
    /// Alice's invite code (for demo mode)
    #[cfg(feature = "development")]
    pub demo_alice_code: String,
    /// Carol's invite code (for demo mode)
    #[cfg(feature = "development")]
    pub demo_carol_code: String,
    /// Mobile device id (for demo MFA shortcuts)
    #[cfg(feature = "development")]
    pub demo_mobile_device_id: String,
    /// Mobile authority id (for demo device enrollment)
    #[cfg(feature = "development")]
    pub demo_mobile_authority_id: String,
    // Reactive update channel - receiver wrapped in Arc<Mutex<Option>> for take-once semantics
    /// UI update receiver for reactive updates from callbacks
    pub update_rx: Option<Arc<Mutex<Option<UiUpdateReceiver>>>>,
    /// Dedicated harness command receiver for semantic command ingress.
    pub harness_command_rx: Option<Arc<Mutex<Option<HarnessCommandReceiver>>>>,
    /// UI update sender for sending updates from event handlers
    pub update_tx: Option<UiUpdateSender>,
    /// Callback registry for all domain actions
    pub callbacks: Option<CallbackRegistry>,
}

/// Main application with screen navigation
#[allow(clippy::field_reassign_with_default)] // Large struct with many conditional fields
#[component]
pub fn IoApp(props: &IoAppProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let screen = hooks.use_state(|| Screen::Neighborhood);
    let mut should_exit = hooks.use_state(|| false);
    let mut system = hooks.use_context_mut::<SystemContext>();

    // Shared shutdown flag for background tasks. Set to true when should_exit
    // transitions, checked by all `use_future` loops to break cleanly.
    let bg_shutdown =
        hooks.use_ref(|| std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)));

    // Pure TUI state machine - holds all UI state for deterministic transitions
    // This is the source of truth; iocraft hooks sync FROM this state
    let show_setup = props.show_account_setup;
    let pending_runtime_bootstrap = props.pending_runtime_bootstrap;
    #[cfg(feature = "development")]
    let demo_alice = props.demo_alice_code.clone();
    #[cfg(feature = "development")]
    let demo_carol = props.demo_carol_code.clone();
    #[cfg(feature = "development")]
    let demo_mobile_device_id = props.demo_mobile_device_id.clone();
    #[cfg(feature = "development")]
    let demo_mobile_authority_id = props.demo_mobile_authority_id.clone();
    let tui_state = hooks.use_ref(move || {
        #[cfg(feature = "development")]
        {
            let mut state = if show_setup {
                TuiState::with_account_setup()
            } else {
                TuiState::new()
            };
            state.pending_runtime_bootstrap = pending_runtime_bootstrap;
            // Set demo mode codes for import modal shortcuts (on contacts screen)
            state.contacts.demo_alice_code = demo_alice.clone();
            state.contacts.demo_carol_code = demo_carol.clone();
            state.settings.demo_mobile_device_id = demo_mobile_device_id.clone();
            state.settings.demo_mobile_authority_id = demo_mobile_authority_id.clone();
            state
        }

        #[cfg(not(feature = "development"))]
        {
            if show_setup {
                let mut state = TuiState::with_account_setup();
                state.pending_runtime_bootstrap = pending_runtime_bootstrap;
                state
            } else {
                let mut state = TuiState::new();
                state.pending_runtime_bootstrap = pending_runtime_bootstrap;
                state
            }
        }
    });
    let tui_state_version = hooks.use_state(|| 0usize);
    let tui = TuiStateHandle::new(tui_state.clone(), tui_state_version.clone());

    // =========================================================================
    // UI Update Channel - Reactive channel for async callback results
    //
    // Callbacks in run_app_with_context send their results through this channel.
    // The update processor (use_future below) awaits on this channel and updates
    // State<T> values, which automatically trigger re-renders via iocraft's waker.
    //
    // The receiver is passed via props.update_rx from run_app_with_context.
    // Typed harness semantic commands use a separate ingress channel so command
    // application/ack is not multiplexed through the broader async update stream.
    // =========================================================================
    let update_rx_holder = props.update_rx.clone();
    let harness_command_rx_holder = props.harness_command_rx.clone();
    let update_tx_holder = props.update_tx.clone();
    let update_tx_for_commands = update_tx_holder.clone();
    let update_tx_for_events = update_tx_holder.clone();

    // Nickname suggestion state - State<T> automatically triggers re-renders on .set()
    let nickname_suggestion_state = hooks.use_state({
        let initial = props.nickname_suggestion.clone();
        move || initial
    });

    // Get AppCoreContext for IoContext access
    let app_ctx = hooks.use_context::<AppCoreContext>();
    let tasks = app_ctx.tasks();
    let app_core_for_events = app_ctx.app_core.raw().clone();

    // =========================================================================
    // NavBar status: derive from reactive signals (no blocking awaits at startup).
    // =========================================================================
    let nav_signals = use_nav_status_signals(
        &mut hooks,
        &app_ctx,
        props.network_status.clone(),
        props.known_online,
        props.transport_peers,
    );
    let projection_export_version = hooks.use_state(|| 0usize);

    // =========================================================================
    // Contacts subscription: SharedContacts for dispatch handlers to read
    // =========================================================================
    // Unlike props.contacts (which is empty), this Arc is kept up-to-date
    // by a reactive subscription. Dispatch handler closures capture the Arc,
    // not the data, so they always read current contacts.
    // Also sends ContactCountChanged updates to keep TuiState in sync for navigation.
    let shared_contacts = use_contacts_subscription(
        &mut hooks,
        &app_ctx,
        update_tx_holder.clone(),
        projection_export_version.clone(),
    );
    let shared_discovered_peers =
        use_discovered_peers_subscription(&mut hooks, &app_ctx, update_tx_holder.clone());

    // =========================================================================
    // Authority subscription: current authority id for dispatch handlers
    // =========================================================================
    let shared_authority_id =
        use_authority_id_subscription(&mut hooks, &app_ctx, update_tx_holder.clone());

    // =========================================================================
    // Shared selected channel identity for subscriptions and dispatch
    // =========================================================================
    let tui_selected_ref =
        hooks.use_ref(|| std::sync::Arc::new(parking_lot::RwLock::new(None::<String>)));
    let tui_selected: std::sync::Arc<parking_lot::RwLock<Option<String>>> =
        tui_selected_ref.read().clone();
    let selected_channel_binding_ref = hooks
        .use_ref(|| std::sync::Arc::new(parking_lot::RwLock::new(None::<SelectedChannelBinding>)));
    let selected_channel_binding: std::sync::Arc<
        parking_lot::RwLock<Option<SelectedChannelBinding>>,
    > = selected_channel_binding_ref.read().clone();
    let last_exported_devices_ref =
        hooks.use_ref(|| std::sync::Arc::new(parking_lot::RwLock::new(Vec::<Device>::new())));
    let last_exported_devices: std::sync::Arc<parking_lot::RwLock<Vec<Device>>> =
        last_exported_devices_ref.read().clone();
    let pending_create_channel_receipts_ref = hooks.use_ref(|| {
        Arc::new(Mutex::new(
            HashMap::<String, HarnessCommandReceiptHandle>::new(),
        ))
    });
    let pending_create_channel_receipts = pending_create_channel_receipts_ref.read().clone();
    let pending_join_channel_receipts_ref = hooks.use_ref(|| {
        Arc::new(Mutex::new(
            HashMap::<String, HarnessCommandReceiptHandle>::new(),
        ))
    });
    let pending_join_channel_receipts = pending_join_channel_receipts_ref.read().clone();
    let ready_join_channel_receipts_ref =
        hooks.use_ref(|| Arc::new(Mutex::new(HashSet::<String>::new())));
    let ready_join_channel_receipts = ready_join_channel_receipts_ref.read().clone();
    let pending_accept_pending_channel_receipts_ref = hooks.use_ref(|| {
        Arc::new(Mutex::new(
            HashMap::<String, HarnessCommandReceiptHandle>::new(),
        ))
    });
    let pending_accept_pending_channel_receipts =
        pending_accept_pending_channel_receipts_ref.read().clone();
    let ready_accept_pending_channel_receipts_ref =
        hooks.use_ref(|| Arc::new(Mutex::new(HashSet::<String>::new())));
    let ready_accept_pending_channel_receipts =
        ready_accept_pending_channel_receipts_ref.read().clone();

    // =========================================================================
    // Channels subscription: SharedChannels for dispatch handlers to read
    // =========================================================================
    // Must be created before messages subscription since messages depend on channels
    let shared_channels = use_channels_subscription(
        &mut hooks,
        &app_ctx,
        shared_authority_id.clone(),
        tui_selected.clone(),
        update_tx_holder.clone(),
        projection_export_version.clone(),
    );

    // =========================================================================
    // Messages subscription: SharedMessages for dispatch handlers to read
    // =========================================================================
    // Used to look up failed messages by ID for retry operations.
    // The Arc is kept up-to-date by a reactive subscription to CHAT_SIGNAL.
    let shared_messages = use_messages_subscription(
        &mut hooks,
        &app_ctx,
        tui_selected.clone(),
        projection_export_version.clone(),
    );

    // Clone for ChatScreen to compute per-channel message counts
    let tui_selected_for_chat_screen = tui_selected.clone();
    let selected_channel_binding_for_chat_screen = selected_channel_binding.clone();

    // =========================================================================
    // Devices subscription: SharedDevices for dispatch handlers to read
    // =========================================================================
    let shared_devices = use_devices_subscription(
        &mut hooks,
        &app_ctx,
        update_tx_holder.clone(),
        projection_export_version.clone(),
    );
    let callbacks_ref =
        hooks.use_ref(|| Arc::new(parking_lot::RwLock::new(props.callbacks.clone())));
    let shared_callbacks = callbacks_ref.read().clone();
    *shared_callbacks.write() = props.callbacks.clone();

    // =========================================================================
    // Invitations subscription: SharedInvitations for notification action dispatch
    // =========================================================================
    let shared_invitations = use_invitations_subscription(
        &mut hooks,
        &app_ctx,
        update_tx_holder.clone(),
        projection_export_version.clone(),
    );
    use_authoritative_semantic_facts_subscription(&mut hooks, &app_ctx, update_tx_holder.clone());

    // =========================================================================
    // Neighborhood homes subscription: SharedNeighborhoodHomes for dispatch handlers to read
    // =========================================================================
    let shared_neighborhood_homes = use_neighborhood_homes_subscription(
        &mut hooks,
        &app_ctx,
        projection_export_version.clone(),
    );
    let shared_neighborhood_home_meta = use_neighborhood_home_meta_subscription(
        &mut hooks,
        &app_ctx,
        projection_export_version.clone(),
    );

    // =========================================================================
    // Pending requests subscription: SharedPendingRequests for dispatch handlers to read
    // =========================================================================
    let shared_pending_requests =
        use_pending_requests_subscription(&mut hooks, &app_ctx, projection_export_version.clone());

    // =========================================================================
    // Notifications subscription: keep notification count in sync for navigation
    // =========================================================================
    use_notifications_subscription(&mut hooks, &app_ctx, update_tx_holder.clone());

    // =========================================================================
    // Threshold subscription: SharedThreshold for dispatch handlers to read
    // =========================================================================
    // Threshold values from settings - used for recovery eligibility checks
    let shared_threshold = use_threshold_subscription(&mut hooks, &app_ctx);
    let shared_threshold_for_dispatch = shared_threshold;

    // =========================================================================
    // ERROR_SIGNAL subscription: central domain error surfacing
    // =========================================================================
    // Rule: AppCore/dispatch failures emit ERROR_SIGNAL (Option<AppError>) and are
    // rendered here (toast queue), so screens/callbacks do not need their own
    // per-operation error toasts.
    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let mut tui = tui.clone();
        let shutdown = bg_shutdown.read().clone();
        async move {
            let format_error = |err: &AppError| format!("{}: {}", err.code(), err);

            // Initial read.
            {
                let reactive = {
                    let core = app_core.raw().read().await;
                    core.reactive().clone()
                };
                if let Ok(Some(err)) = reactive.read(&*ERROR_SIGNAL).await {
                    let msg = format_error(&err);
                    tui.with_mut(|state| {
                        // Prefer routing errors into the account setup modal when it is active.
                        let routed = matches!(
                            state.modal_queue.current(),
                            Some(QueuedModal::AccountSetup(_))
                        );
                        if routed {
                            state.modal_queue.update_active(|modal| {
                                if let QueuedModal::AccountSetup(ref mut s) = modal {
                                    s.set_error(msg.clone());
                                }
                            });
                        }

                        if !routed {
                            let toast_id = state.next_toast_id;
                            state.next_toast_id += 1;
                            let toast = crate::tui::state::QueuedToast::new(
                                toast_id,
                                msg,
                                crate::tui::state::ToastLevel::Error,
                            );
                            state.toast_queue.enqueue(toast);
                        }
                    });
                }
            }

            let retry_policy = shell_retry_policy();
            let time = PhysicalTimeHandler::new();
            let result = execute_with_retry_budget(&time, &retry_policy, |_attempt| {
                let app_core = app_core.clone();
                let mut tui = tui.clone();
                let shutdown = shutdown.clone();
                async move {
                    if shutdown.load(std::sync::atomic::Ordering::Acquire) {
                        return Ok::<(), String>(());
                    }

                    let mut stream = {
                        let core = app_core.raw().read().await;
                        core.subscribe(&*ERROR_SIGNAL)
                            .map_err(|e| format!("error signal subscription failed: {e}"))?
                    };

                    while let Ok(err_opt) = stream.recv().await {
                        if shutdown.load(std::sync::atomic::Ordering::Acquire) {
                            return Ok(());
                        }
                        let Some(err) = err_opt else { continue };
                        let msg = format_error(&err);
                        tui.with_mut(|state| {
                            // Prefer routing errors into the account setup modal when it is active.
                            let routed = matches!(
                                state.modal_queue.current(),
                                Some(QueuedModal::AccountSetup(_))
                            );
                            if routed {
                                state.modal_queue.update_active(|modal| {
                                    if let QueuedModal::AccountSetup(ref mut s) = modal {
                                        s.set_error(msg.clone());
                                    }
                                });
                            }

                            if !routed {
                                let toast_id = state.next_toast_id;
                                state.next_toast_id += 1;
                                let toast = crate::tui::state::QueuedToast::new(
                                    toast_id,
                                    msg,
                                    crate::tui::state::ToastLevel::Error,
                                );
                                state.toast_queue.enqueue(toast);
                            }
                        });
                    }

                    Err("ERROR_SIGNAL subscription stream ended".to_string())
                }
            })
            .await;

            match result {
                Ok(()) => {}
                Err(RetryRunError::AttemptsExhausted {
                    attempts_used,
                    last_error,
                }) => tracing::warn!(
                    attempts_used,
                    last_error,
                    "ERROR_SIGNAL subscription abandoned after max retries"
                ),
                Err(RetryRunError::Timeout(error)) => tracing::warn!(
                    error = %error,
                    "ERROR_SIGNAL retry budget timed out"
                ),
            }
        }
    });

    // =========================================================================
    // Toast Auto-Dismiss Timer
    //
    // Runs every 100ms to tick the toast queue, enabling auto-dismiss for
    // non-error toasts (5 second timeout). Error toasts never auto-dismiss.
    // Only triggers re-render when a toast is actually dismissed.
    // =========================================================================
    hooks.use_future({
        let mut tui = tui.clone();
        let shutdown = bg_shutdown.read().clone();
        async move {
            loop {
                effect_sleep(std::time::Duration::from_millis(100)).await;
                if shutdown.load(std::sync::atomic::Ordering::Acquire) {
                    break;
                }
                // Only tick auto-dismissing toasts. Keep error toasts static and avoid
                // forcing a full re-render unless dismissal actually occurred.
                let should_tick = tui
                    .read_clone()
                    .toast_queue
                    .current()
                    .is_some_and(|toast| toast.auto_dismisses());
                if should_tick {
                    tui.tick_active_toast_timer();
                }
            }
        }
    });

    // =========================================================================
    // Discovered Peers Auto-Refresh
    //
    // Keep LAN/rendezvous peer discovery fresh in the background so the
    // Contacts screen can stay purely reactive.
    // =========================================================================
    hooks.use_future({
        let app_core = app_ctx.app_core.raw().clone();
        let shutdown = bg_shutdown.read().clone();
        async move {
            loop {
                if shutdown.load(std::sync::atomic::Ordering::Acquire) {
                    break;
                }
                let timestamp_ms =
                    match aura_app::ui::workflows::time::current_time_ms(&app_core).await {
                        Ok(ts) => ts,
                        Err(e) => {
                            tracing::debug!(error = %e, "current_time_ms failed in peer discovery");
                            0
                        }
                    };
                if let Err(e) = network_workflows::discover_peers(&app_core, timestamp_ms).await {
                    tracing::debug!(error = %e, "discover_peers failed");
                }
                effect_sleep(network_workflows::DISCOVERED_PEERS_REFRESH_INTERVAL).await;
            }
        }
    });

    // =========================================================================
    // Harness Runtime Maintenance
    //
    // In harness mode, keep ceremony ingestion, sync, and discovery moving while
    // the UI is idle on a screen. Shared-flow receive steps otherwise depend too
    // heavily on incidental user actions to drive runtime convergence.
    // =========================================================================
    if harness_mode_enabled() {
        hooks.use_future({
            let app_core = app_ctx.app_core.raw().clone();
            let shutdown = bg_shutdown.read().clone();
            async move {
                loop {
                    if shutdown.load(std::sync::atomic::Ordering::Acquire) {
                        break;
                    }
                    let runtime = {
                        let core = app_core.read().await;
                        core.runtime().cloned()
                    };

                    if let Some(runtime) = runtime {
                        let _ = runtime.trigger_discovery().await;
                        let _ = runtime.process_ceremony_messages().await;
                        let _ = runtime.trigger_sync().await;
                        let _ = runtime.process_ceremony_messages().await;
                    }

                    let _ = system_workflows::refresh_account(&app_core).await;

                    effect_sleep(tokio::time::Duration::from_secs(1)).await;
                }
            }
        });
    }

    // =========================================================================
    // UI Update Processor - Central handler for all async callback results

    // This is the single point where all async updates flow through.
    // Callbacks send UiUpdate variants, this processor matches and updates
    // the appropriate State<T> values, triggering automatic re-renders.
    // Only runs if update_rx was provided via props.
    // =========================================================================
    let tasks_for_updates = tasks.clone();
    if let Some(command_rx_holder) = harness_command_rx_holder {
        hooks.use_future({
            let mut screen = screen.clone();
            let mut should_exit = should_exit.clone();
            let app_ctx_for_commands = app_ctx.clone();
            let shared_callbacks_for_commands = shared_callbacks;
            let mut tui = tui.clone();
            let shared_contacts_for_commands = shared_contacts.clone();
            let shared_channels_for_commands = shared_channels.clone();
            let shared_devices_for_commands = shared_devices.clone();
            let shared_messages_for_commands = shared_messages.clone();
            let last_exported_devices_for_commands = last_exported_devices.clone();
            let tui_selected_for_commands = tui_selected_for_chat_screen.clone();
            let selected_channel_binding_for_commands =
                selected_channel_binding_for_chat_screen.clone();
            let pending_create_channel_receipts_for_commands =
                pending_create_channel_receipts.clone();
            let pending_join_channel_receipts_for_commands =
                pending_join_channel_receipts.clone();
            let pending_accept_pending_channel_receipts_for_commands =
                pending_accept_pending_channel_receipts.clone();
            async move {
                #[allow(clippy::expect_used)]
                let mut rx = {
                    let mut guard = command_rx_holder
                        .lock()
                        .expect("Failed to lock harness_command_rx");
                    guard
                        .take()
                        .expect("Harness command receiver already taken")
                };

                while let Some(submission) = rx.recv().await {
                    let app_snapshot_for_command = app_ctx_for_commands.snapshot();
                    let harness_contacts_for_command = shared_contacts_for_commands.read().clone();
                    let harness_channels_for_command = shared_channels_for_commands.read().clone();
                    let harness_devices_for_command =
                        authoritative_settings_devices_for_command(
                            &app_ctx_for_commands,
                            &shared_devices_for_commands,
                        )
                        .await;
                    let (authorities_for_command, current_authority_index_for_command) =
                        authoritative_settings_authorities_for_command(&app_ctx_for_commands).await;
                    let harness_messages_for_command = shared_messages_for_commands.read().clone();

                    let apply_result = tui.with_mut(|state| {
                        let callbacks_for_commands = shared_callbacks_for_commands.read().clone();
                        let mut operation_handle = None;
                        if !authorities_for_command.is_empty() {
                            state.authorities = authorities_for_command.clone();
                            state.current_authority_index = current_authority_index_for_command
                                .min(state.authorities.len().saturating_sub(1));
                        }
                        let followup = apply_harness_command(
                            state,
                            submission.command.clone(),
                            TuiSemanticInputs {
                                app_snapshot: &app_snapshot_for_command,
                                contacts: &harness_contacts_for_command,
                                settings_devices: &harness_devices_for_command,
                                chat_channels: &harness_channels_for_command,
                                chat_messages: &harness_messages_for_command,
                            },
                        )?;
                        for command in followup {
                            if let Some(handle) = execute_harness_followup_command(
                                state,
                                command,
                                &callbacks_for_commands,
                                &app_ctx_for_commands,
                                &update_tx_for_commands,
                                &shared_contacts_for_commands,
                                &shared_channels_for_commands,
                                &shared_devices_for_commands,
                                &shared_messages_for_commands,
                                &last_exported_devices_for_commands,
                                &tui_selected_for_commands,
                                &selected_channel_binding_for_commands,
                            )? {
                                operation_handle = Some(handle);
                            }
                        }
                        Ok::<_, String>((state.screen(), operation_handle))
                    });
                    let (next_screen, operation_handle) = match apply_result {
                        Ok(result) => result,
                        Err(error) => {
                            submission.receipt.complete(
                                aura_app::ui::contract::HarnessUiCommandReceipt::Rejected {
                                    reason: error,
                                },
                            );
                            continue;
                        }
                    };
                    if next_screen != screen.get() {
                        screen.set(next_screen);
                    }

                    let updated_state = tui.read_clone();
                    if updated_state.should_exit && !should_exit.get() {
                        should_exit.set(true);
                        bg_shutdown.read().store(true, std::sync::atomic::Ordering::Release);
                    }

                    let app_snapshot = app_ctx_for_commands.snapshot();
                    let harness_contacts = shared_contacts_for_commands.read().clone();
                    let harness_devices = shared_devices_for_commands.read().clone();
                    let harness_channels = shared_channels_for_commands.read().clone();
                    let harness_messages = shared_messages_for_commands.read().clone();
                    let defer_create_channel_receipt = matches!(
                        submission.command,
                        HarnessUiCommand::CreateChannel { .. }
                    ) && operation_handle.is_some();
                    let defer_join_channel_receipt = matches!(
                        submission.command,
                        HarnessUiCommand::JoinChannel { .. }
                    ) && operation_handle.is_some();
                    let defer_accept_pending_channel_receipt = matches!(
                        submission.command,
                        HarnessUiCommand::AcceptPendingChannelInvitation
                    ) && operation_handle.is_some();
                    if defer_create_channel_receipt
                        || defer_join_channel_receipt
                        || defer_accept_pending_channel_receipt
                    {
                        let Some(handle) = operation_handle.clone() else {
                            submission.receipt.complete(
                                aura_app::ui::contract::HarnessUiCommandReceipt::Rejected {
                                    reason: "semantic command did not expose a canonical ui operation handle with exact instance tracking".to_string(),
                                },
                            );
                            continue;
                        };
                        let pending_receipts = if defer_create_channel_receipt {
                            &pending_create_channel_receipts_for_commands
                        } else if defer_join_channel_receipt {
                            &pending_join_channel_receipts_for_commands
                        } else {
                            &pending_accept_pending_channel_receipts_for_commands
                        };
                        pending_receipts
                            .lock()
                            .unwrap()
                            .insert(handle.instance_id().0.clone(), submission.receipt.clone());
                    } else {
                        let receipt = match operation_handle {
                            Some(operation) => {
                                aura_app::ui::contract::HarnessUiCommandReceipt::AcceptedWithOperation {
                                    operation,
                                    value: None,
                                }
                            }
                            None => {
                                aura_app::ui::contract::HarnessUiCommandReceipt::Accepted {
                                    value: None,
                                }
                            }
                        };
                        submission.receipt.complete(receipt);
                    }

                    let export_result = maybe_export_ui_snapshot(
                        &updated_state,
                        TuiSemanticInputs {
                            app_snapshot: &app_snapshot,
                            contacts: &harness_contacts,
                            settings_devices: &harness_devices,
                            chat_channels: &harness_channels,
                            chat_messages: &harness_messages,
                        },
                    );
                    if let Err(error) = export_result {
                        tracing::warn!(
                            error = %error,
                            "failed to export authoritative TUI projection after applying harness command"
                        );
                    }
                }
            }
        });
    }
    if let Some(rx_holder) = update_rx_holder {
        hooks.use_future({
            let mut nickname_suggestion_state = nickname_suggestion_state.clone();
            let app_core = app_ctx.app_core.clone();
            let app_ctx_for_updates = app_ctx.clone();
            // Toast queue migration: mutate TuiState via TuiStateHandle (always bumps render version)
            let mut tui = tui.clone();
            let shared_contacts_for_updates = shared_contacts.clone();
            let shared_channels_for_updates = shared_channels.clone();
            let shared_devices_for_updates = shared_devices.clone();
            let shared_messages_for_updates = shared_messages.clone();
            // Shared selection state for messages subscription synchronization
            let tui_selected_for_updates = tui_selected;
            let selected_channel_binding_for_updates = selected_channel_binding;
            let pending_create_channel_receipts_for_updates = pending_create_channel_receipts;
            let pending_join_channel_receipts_for_updates = pending_join_channel_receipts;
            let ready_join_channel_receipts_for_updates = ready_join_channel_receipts;
            let pending_accept_pending_channel_receipts_for_updates =
                pending_accept_pending_channel_receipts;
            let ready_accept_pending_channel_receipts_for_updates =
                ready_accept_pending_channel_receipts;
            async move {
                // Helper macro-like function to add a toast to the queue
                // (Inline to avoid borrow checker issues with closures)
                macro_rules! enqueue_toast {
                    ($msg:expr, $level:expr) => {{
                        tui.with_mut(|state| {
                            let toast_id = state.next_toast_id;
                            state.next_toast_id += 1;
                            let toast = crate::tui::state::QueuedToast::new(
                                toast_id,
                                $msg,
                                $level,
                            );
                            state.toast_queue.enqueue(toast);
                        });
                    }};
                }

                // Take the receiver from the holder (only happens once)
                #[allow(clippy::expect_used)]
                // TUI initialization - panic is appropriate if channel setup failed
                let mut rx = {
                    let mut guard = rx_holder.lock().expect("Failed to lock update_rx");
                    guard.take().expect("UI update receiver already taken")
                };

                // Process updates as they arrive
                while let Some(update) = rx.recv().await {
                    // IMPORTANT: This match is intentionally exhaustive (no `_ => {}`).
                    // Adding a new UiUpdate variant must cause a compile-time error here,
                    // so the shell cannot silently drop UI updates.
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
                                state.current_authority_index = current_index
                                    .min(state.authorities.len().saturating_sub(1));
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
                                state.settings.last_device_enrollment_code =
                                    enrollment_code.clone();
                                state.upsert_runtime_fact(RuntimeFact::DeviceEnrollmentCodeReady {
                                    device_name: Some(nickname_suggestion.clone()),
                                    code_len: Some(enrollment_code.len()),
                                    code: Some(enrollment_code.clone()),
                                });
                                if state.settings.pending_mobile_enrollment_autofill {
                                    state.settings.pending_mobile_enrollment_autofill = false;
                                    state.modal_queue.update_active(|modal| {
                                        if let crate::tui::state::QueuedModal::SettingsDeviceImport(ref mut s) = modal {
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
                            let mut toast: Option<(String, crate::tui::state::ToastLevel)> =
                                None;
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
                                        error_message
                                            .clone()
                                            .unwrap_or_else(|| "Device enrollment failed".to_string()),
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
                            // Convert ToastMessage to QueuedToast and enqueue.
                            let level = match toast.level {
                                ToastLevel::Info => crate::tui::state::ToastLevel::Info,
                                ToastLevel::Success => {
                                    crate::tui::state::ToastLevel::Success
                                }
                                ToastLevel::Warning => {
                                    crate::tui::state::ToastLevel::Warning
                                }
                                ToastLevel::Error | ToastLevel::Conflict => {
                                    crate::tui::state::ToastLevel::Error
                                }
                            };
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
                                let visible_message_channel = messages
                                    .last()
                                    .map(|message| message.channel_id.clone());
                                let should_append = selected_channel.as_deref()
                                    == Some(channel.as_str())
                                    || state_selected_channel.as_deref()
                                        == Some(channel.as_str())
                                    || visible_message_channel.as_deref()
                                        == Some(channel.as_str());
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
                                            channel.clone(),
                                            "You",
                                            content.clone(),
                                        ));
                                        appended = true;
                                    }
                                }
                            }
                            // Auto-scroll to bottom (show latest messages including the one just sent)
                            tui.with_mut(|state| {
                                if appended {
                                    state.chat.message_count =
                                        state.chat.message_count.saturating_add(1);
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
                        UiUpdate::ChannelSelected(channel_id) => {
                            let selected_channel = shared_channels_for_updates
                                .read()
                                .iter()
                                .enumerate()
                                .find_map(|(idx, channel)| {
                                    (channel.id == channel_id).then_some((idx, channel.clone()))
                                });
                            *tui_selected_for_updates.write() = Some(channel_id.clone());
                            {
                                let mut guard = selected_channel_binding_for_updates.write();
                                let previous = guard.clone();
                                *guard = selected_channel.as_ref().map(|(_, channel)| {
                                    SelectedChannelBinding::merged_from_channel(
                                        channel,
                                        previous.as_ref(),
                                    )
                                });
                            }
                            if let Some((idx, channel)) = selected_channel {
                                tui.with_mut(|state| {
                                    state.chat.selected_channel = idx;
                                    state.chat.message_scroll = 0;
                                    let _ = &channel;
                                });
                            }
                            if let Some(binding) =
                                selected_channel_binding_for_updates.read().clone()
                            {
                                complete_ready_channel_binding_receipts(
                                    &pending_join_channel_receipts_for_updates,
                                    &ready_join_channel_receipts_for_updates,
                                    aura_app::ui_contract::OperationId::join_channel(),
                                    &binding,
                                );
                                complete_ready_channel_binding_receipts(
                                    &pending_accept_pending_channel_receipts_for_updates,
                                    &ready_accept_pending_channel_receipts_for_updates,
                                    aura_app::ui_contract::OperationId::invitation_accept(),
                                    &binding,
                                );
                            }
                        }
                        UiUpdate::ChannelCreated {
                            operation_instance_id,
                            channel_id,
                            context_id,
                            name,
                        } => {
                            *tui_selected_for_updates.write() = Some(channel_id.clone());
                            *selected_channel_binding_for_updates.write() = Some(SelectedChannelBinding {
                                channel_id: channel_id.clone(),
                                context_id: context_id.clone(),
                            });
                            {
                                let mut channels = shared_channels_for_updates.write();
                                if let Some(channel) =
                                    channels.iter_mut().find(|channel| channel.id == channel_id)
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
                            let selected = shared_channels_for_updates.read()
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
                                    if let Some(receipt) = pending_create_channel_receipts_for_updates
                                        .lock()
                                        .unwrap()
                                        .remove(&instance_id.0)
                                    {
                                    receipt.complete(
                                        aura_app::ui::contract::HarnessUiCommandReceipt::AcceptedWithOperation {
                                            operation: aura_app::ui_contract::HarnessUiOperationHandle::new(
                                                aura_app::ui_contract::OperationId::create_channel(),
                                                instance_id,
                                            ),
                                            value: Some(
                                                aura_app::scenario_contract::SemanticCommandValue::ChannelBinding {
                                                    channel_id,
                                                    context_id,
                                                },
                                            ),
                                        },
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
                                let committed_index = committed_selection.as_ref().and_then(|selected_id| {
                                    shared_channels_for_updates
                                        .read()
                                        .iter()
                                        .position(|channel| channel.id == *selected_id)
                                });

                                if let Some(idx) = committed_index {
                                    state.chat.selected_channel = idx;
                                } else if committed_selection.is_none()
                                    && state.chat.selected_channel >= channel_count
                                {
                                    let idx =
                                        clamp_list_index(selected_index.unwrap_or(0), channel_count);
                                    state.chat.selected_channel = idx;
                                    state.chat.message_scroll = 0;
                                }

                                *tui_selected_for_updates.write() = shared_channels_for_updates
                                    .read()
                                    .get(state.chat.selected_channel)
                                    .map(|channel| channel.id.clone());
                                {
                                    let mut guard = selected_channel_binding_for_updates.write();
                                    let previous = guard.clone();
                                    *guard = shared_channels_for_updates
                                        .read()
                                        .get(state.chat.selected_channel)
                                        .map(|channel| {
                                            SelectedChannelBinding::merged_from_channel(
                                                channel,
                                                previous.as_ref(),
                                            )
                                        });
                                }

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
                            if let Some(binding) =
                                selected_channel_binding_for_updates.read().clone()
                            {
                                complete_ready_channel_binding_receipts(
                                    &pending_join_channel_receipts_for_updates,
                                    &ready_join_channel_receipts_for_updates,
                                    aura_app::ui_contract::OperationId::join_channel(),
                                    &binding,
                                );
                                complete_ready_channel_binding_receipts(
                                    &pending_accept_pending_channel_receipts_for_updates,
                                    &ready_accept_pending_channel_receipts_for_updates,
                                    aura_app::ui_contract::OperationId::invitation_accept(),
                                    &binding,
                                );
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
                                        if let Some(contact) =
                                            contacts.iter().find(|c| c.id == *entry)
                                        {
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
                                            && (mapped_participants.len() > 1 || info.participants.len() <= 1) {
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
                        UiUpdate::InvitationExported { code } => {
                            tui.with_mut(|state| {
                                state.last_exported_invitation_code = Some(code.clone());
                                state.upsert_runtime_fact(RuntimeFact::InvitationCodeReady {
                                    receiver_authority_id: None,
                                    source_operation: OperationId::invitation_create(),
                                    code: Some(code.clone()),
                                });
                                let copied = copy_to_clipboard(&code).is_ok();
                                state
                                    .modal_queue
                                    .enqueue(crate::tui::state::QueuedModal::ContactsCode(
                                        {
                                            let mut modal =
                                                crate::tui::state::InvitationCodeModalState::for_code(code);
                                            if copied {
                                                modal.set_copied();
                                            }
                                            modal
                                        },
                                    ));
                            });
                        }
                        UiUpdate::AuthoritativeOperationStatus {
                            operation_id,
                            status,
                            instance_id,
                            causality,
                        } => {
                            if operation_id == aura_app::ui_contract::OperationId::create_channel()
                                && matches!(
                                    status.phase,
                                    aura_app::ui_contract::SemanticOperationPhase::Failed
                                        | aura_app::ui_contract::SemanticOperationPhase::Cancelled
                                )
                            {
                                if let Some(instance_id) = instance_id.clone() {
                                    if let Some(receipt) =
                                        pending_create_channel_receipts_for_updates
                                            .lock()
                                            .unwrap()
                                            .remove(&instance_id.0)
                                    {
                                        let reason = status
                                            .error
                                            .as_ref()
                                            .and_then(|error| error.detail.clone())
                                            .unwrap_or_else(|| "create channel failed".to_string());
                                        receipt.complete(
                                            aura_app::ui::contract::HarnessUiCommandReceipt::Rejected {
                                                reason,
                                            },
                                        );
                                    }
                                }
                            }
                            if let Some(instance_id) = instance_id.clone() {
                                let is_join_channel = status.kind
                                    == SemanticOperationKind::JoinChannel
                                    && operation_id
                                        == aura_app::ui_contract::OperationId::join_channel();
                                let is_accept_pending_channel = status.kind
                                    == SemanticOperationKind::AcceptPendingChannelInvitation
                                    && operation_id
                                        == aura_app::ui_contract::OperationId::invitation_accept();
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
                                        ready_join_channel_receipts_for_updates
                                            .lock()
                                            .unwrap()
                                            .remove(&instance_id.0);
                                        if let Some(receipt) = pending_join_channel_receipts_for_updates
                                            .lock()
                                            .unwrap()
                                            .remove(&instance_id.0)
                                        {
                                            let reason = status
                                                .error
                                                .as_ref()
                                                .and_then(|error| error.detail.clone())
                                                .unwrap_or_else(|| "join channel failed".to_string());
                                            receipt.complete(
                                                aura_app::ui::contract::HarnessUiCommandReceipt::Rejected {
                                                    reason,
                                                },
                                            );
                                        }
                                    } else if is_succeeded {
                                        ready_join_channel_receipts_for_updates
                                            .lock()
                                            .unwrap()
                                            .insert(instance_id.0.clone());
                                        if let Some(binding) =
                                            selected_channel_binding_for_updates.read().clone()
                                        {
                                            complete_ready_channel_binding_receipts(
                                                &pending_join_channel_receipts_for_updates,
                                                &ready_join_channel_receipts_for_updates,
                                                aura_app::ui_contract::OperationId::join_channel(),
                                                &binding,
                                            );
                                        }
                                    }
                                }
                                if is_accept_pending_channel {
                                    if is_failed_or_cancelled {
                                        ready_accept_pending_channel_receipts_for_updates
                                            .lock()
                                            .unwrap()
                                            .remove(&instance_id.0);
                                        if let Some(receipt) = pending_accept_pending_channel_receipts_for_updates
                                            .lock()
                                            .unwrap()
                                            .remove(&instance_id.0)
                                        {
                                            let reason = status
                                                .error
                                                .as_ref()
                                                .and_then(|error| error.detail.clone())
                                                .unwrap_or_else(|| "accept pending channel invitation failed".to_string());
                                            receipt.complete(
                                                aura_app::ui::contract::HarnessUiCommandReceipt::Rejected {
                                                    reason,
                                                },
                                            );
                                        }
                                    } else if is_succeeded {
                                        ready_accept_pending_channel_receipts_for_updates
                                            .lock()
                                            .unwrap()
                                            .insert(instance_id.0.clone());
                                        if let Some(binding) =
                                            selected_channel_binding_for_updates.read().clone()
                                        {
                                            complete_ready_channel_binding_receipts(
                                                &pending_accept_pending_channel_receipts_for_updates,
                                                &ready_accept_pending_channel_receipts_for_updates,
                                                aura_app::ui_contract::OperationId::invitation_accept(),
                                                &binding,
                                            );
                                        }
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
                                aura_app::ui_contract::SemanticOperationPhase::Failed => {
                                    OperationState::Failed
                                }
                                aura_app::ui_contract::SemanticOperationPhase::Cancelled => {
                                    OperationState::Failed
                                }
                                aura_app::ui_contract::SemanticOperationPhase::Succeeded => {
                                    OperationState::Succeeded
                                }
                                _ => OperationState::Submitting,
                            };
                            tui.with_mut(|state| {
                                state.set_authoritative_operation_state(
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
                                enqueue_toast!(
                                    message,
                                    crate::tui::state::ToastLevel::Error
                                );
                            }
                            let export_state = tui.read_clone();
                            let app_snapshot = app_ctx_for_updates.snapshot();
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
                            let mut toast: Option<(String, crate::tui::state::ToastLevel)> =
                                None;
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
                        UiUpdate::RuntimeFactsUpdated {
                            replace_kinds,
                            facts,
                        } => {
                            tui.with_mut(|state| {
                                for kind in replace_kinds {
                                    state.clear_runtime_fact_kind(kind);
                                }
                                for fact in facts {
                                    state.upsert_runtime_fact(fact);
                                }
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
                                        state.contacts.list_focus =
                                            crate::tui::state::ContactsListFocus::Contacts;
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
                            tui.with_mut(|state| {
                                state.account_created_queued();
                            });
                            if show_setup {
                                should_exit.set(true);
                                bg_shutdown.read().store(true, std::sync::atomic::Ordering::Release);
                            }
                        }

                        // =========================================================================
                        // Sync
                        // =========================================================================
                        UiUpdate::SyncStarted => {
                            enqueue_toast!(
                                "Syncing…".to_string(),
                                crate::tui::state::ToastLevel::Info
                            );
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
                                enqueue_toast!(
                                    message,
                                    toast_level
                                );
                            }
                        }

                    }
                }
            }
        });
    }

    // Handle exit request
    if should_exit.get() {
        system.exit();
    }

    // Note: Domain data (channels, messages, guardians, etc.) is no longer passed to screens.
    // Each screen subscribes to signals directly via AppCoreContext.
    // See scripts/check/arch.sh --reactive for architectural enforcement.

    // Read TUI state for rendering via type-safe handle.
    // This MUST be used for all render-time state access - it reads the version to establish
    // reactivity, ensuring the component re-renders when state changes via tui.replace().
    // See TuiStateHandle and TuiStateSnapshot docs for the reactivity model.
    let tui_snapshot = tui.read_for_render();
    let _projection_export_version = projection_export_version.get();
    let app_snapshot = app_ctx.snapshot();
    let harness_devices = shared_devices.read().clone();
    let harness_contacts = shared_contacts.read().clone();
    let harness_channels = shared_channels.read().clone();
    let harness_messages = shared_messages.read().clone();
    if !harness_devices.is_empty() {
        *last_exported_devices.write() = harness_devices.clone();
    }
    if let Err(error) = maybe_export_ui_snapshot(
        &tui_snapshot,
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
            "failed to publish TUI harness snapshot during render"
        );
    }
    // Callbacks registry and individual callback extraction for screen props
    let callbacks = props.callbacks.clone();

    // Extract individual callbacks from registry for screen component props
    // (Screen components still use individual callback props for now)
    let on_send = callbacks.as_ref().map(|cb| cb.chat.on_send.clone());
    let on_retry_message = callbacks
        .as_ref()
        .map(|cb| cb.chat.on_retry_message.clone());
    let on_create_channel = callbacks
        .as_ref()
        .map(|cb| cb.chat.on_create_channel.clone());
    let on_set_topic = callbacks.as_ref().map(|cb| cb.chat.on_set_topic.clone());

    let on_update_nickname = callbacks
        .as_ref()
        .map(|cb| cb.contacts.on_update_nickname.clone());
    let on_start_chat = callbacks
        .as_ref()
        .map(|cb| cb.contacts.on_start_chat.clone());
    let on_invite_lan_peer = callbacks
        .as_ref()
        .map(|cb| cb.contacts.on_invite_lan_peer.clone());
    let on_update_mfa = callbacks
        .as_ref()
        .map(|cb| cb.settings.on_update_mfa.clone());
    let on_update_nickname_suggestion = callbacks
        .as_ref()
        .map(|cb| cb.settings.on_update_nickname_suggestion.clone());
    let on_update_threshold = callbacks
        .as_ref()
        .map(|cb| cb.settings.on_update_threshold.clone());
    let on_add_device = callbacks
        .as_ref()
        .map(|cb| cb.settings.on_add_device.clone());
    let on_remove_device = callbacks
        .as_ref()
        .map(|cb| cb.settings.on_remove_device.clone());

    let current_screen = screen.get();

    // Check if in insert mode (MessageInput has its own hint bar, so hide main hints)
    // Note: tui_snapshot was created earlier during render for all render-time state access
    let is_insert_mode = tui_snapshot.is_insert_mode();

    // Extract screen view props from TuiState using testable extraction functions
    let chat_props = extract_chat_view_props(&tui_snapshot);
    let contacts_props = extract_contacts_view_props(&tui_snapshot);
    let settings_props = extract_settings_view_props(&tui_snapshot);
    let notifications_props = extract_notifications_view_props(&tui_snapshot);
    let neighborhood_props = extract_neighborhood_view_props(&tui_snapshot);

    // =========================================================================
    // Global modal overlays
    // =========================================================================
    let global_modals = build_global_modals(current_screen, &tui_snapshot);

    // Extract toast state from queue (type-enforced single toast at a time)
    let queued_toast = tui_snapshot.toast_queue.current().cloned();

    // Global/screen hints come from one shared keybinding registry.
    let global_hints = global_footer_hints();
    let screen_hints = screen_footer_hints(current_screen);

    let state_indicator = state_indicator_label(&tui_snapshot);

    let tasks_for_events = tasks.clone();
    hooks.use_terminal_events({
        let mut screen = screen.clone();
        let mut should_exit = should_exit.clone();
        let mut tui = tui;
        // Clone AppCore for key rotation operations
        let app_core_for_ceremony = app_ctx.app_core.clone();
        let io_ctx_for_ceremony = app_ctx.clone();
        let _tasks_for_dispatch = tasks;
        // Clone update channel sender for ceremony UI updates
        let update_tx_for_ceremony = props.update_tx.clone();
        // Clone callbacks registry for command dispatch
        let callbacks = callbacks.clone();
        // Clone shared contacts Arc for guardian setup dispatch
        let shared_channels_for_dispatch = shared_channels.clone();
        let shared_neighborhood_homes_for_dispatch = shared_neighborhood_homes;
        let shared_neighborhood_home_meta_for_dispatch = shared_neighborhood_home_meta;
        let shared_invitations_for_dispatch = shared_invitations;
        let shared_pending_requests_for_dispatch = shared_pending_requests;
        // This Arc is updated by a reactive subscription, so reading from it
        // always gets current contacts (not stale props)
        let shared_contacts_for_dispatch = shared_contacts;
        let shared_discovered_peers_for_dispatch = shared_discovered_peers;
        let _shared_authority_id_for_dispatch = shared_authority_id;
        let app_ctx_for_dispatch = app_ctx.clone();
        // Clone shared messages Arc for message retry dispatch
        // Used to look up failed messages by ID to get channel and content for retry
        let shared_messages_for_dispatch = shared_messages.clone();
        // Used to map device selection for MFA wizard
        let shared_devices_for_dispatch = shared_devices;
        // Clone shared selection state for immediate sync on channel navigation
        let tui_selected_for_events = tui_selected_for_chat_screen.clone();
        let selected_channel_binding_for_events = selected_channel_binding_for_chat_screen;
        // Used for recovery eligibility checks (from threshold subscription)
        move |event| {
            if let Some(input_transition) = transition_from_terminal_event(
                event,
                &tui,
                &shared_channels_for_dispatch,
                &shared_neighborhood_homes_for_dispatch,
                &shared_neighborhood_home_meta_for_dispatch,
            ) {
                let current = input_transition.current;
                let mut new_state = input_transition.new_state;
                let commands = input_transition.commands;

                // Execute commands using callbacks registry
                if let Some(ref cb) = callbacks {
                    handle_channel_selection_change(
                        &current,
                        &new_state,
                        &shared_channels_for_dispatch,
                        &tui_selected_for_events,
                        &selected_channel_binding_for_events,
                    );
                    for cmd in commands {
                        match cmd {
                            TuiCommand::Exit => {
                                should_exit.set(true);
                                bg_shutdown.read().store(true, std::sync::atomic::Ordering::Release);
                            }
                            TuiCommand::HarnessRemoveVisibleDevice { device_id } => {
                                let current_devices = shared_devices_for_dispatch.read().clone();
                                let Some(device_id) = device_id
                                    .or_else(|| {
                                        current_devices
                                            .iter()
                                            .find(|device| !device.is_current)
                                            .map(|device| device.id.clone())
                                    })
                                    .or_else(|| {
                                        (current_devices.len() > 1)
                                            .then(|| {
                                                current_devices
                                                    .last()
                                                    .map(|device| device.id.clone())
                                            })
                                            .flatten()
                                    })
                                else {
                                    new_state.toast_error("No removable device is visible");
                                    continue;
                                };
                                (cb.settings.on_remove_device)(device_id.into());
                            }
                            TuiCommand::Dispatch(dispatch_cmd) => {
                                // Handle dispatch commands via CallbackRegistry
                                match dispatch_cmd {
                                    DispatchCommand::CreateAccount { name } => {
                                        let Some(update_tx) = update_tx_for_events.clone() else {
                                            new_state.toast_error(
                                                "UI update sender is unavailable",
                                            );
                                            continue;
                                        };
                                        let operation = LocalTerminalOperationOwner::submit(
                                            app_core_for_events.clone(),
                                            tasks_for_events.clone(),
                                            update_tx,
                                            OperationId::account_create(),
                                            SemanticOperationKind::CreateAccount,
                                        );
                                        (cb.app.on_create_account)(name, operation);
                                    }
                                    DispatchCommand::ImportDeviceEnrollmentDuringOnboarding {
                                        code,
                                    } => {
                                        let Some(update_tx) = update_tx_for_events.clone() else {
                                            new_state.toast_error(
                                                "UI update sender is unavailable",
                                            );
                                            continue;
                                        };
                                        let operation = LocalTerminalOperationOwner::submit(
                                            app_core_for_events.clone(),
                                            tasks_for_events.clone(),
                                            update_tx,
                                            OperationId::device_enrollment(),
                                            SemanticOperationKind::ImportDeviceEnrollmentCode,
                                        );
                                        (cb.app.on_import_device_enrollment_during_onboarding)(
                                            code,
                                            operation,
                                        );
                                    }
                                    DispatchCommand::AddGuardian { contact_id } => {
                                        (cb.recovery.on_select_guardian)(contact_id.to_string());
                                    }

                                    // === Chat Screen Commands ===
                                    DispatchCommand::SelectChannel { channel_id } => {
                                        let channels = shared_channels_for_dispatch.read().clone();
                                        if let Some(idx) = channels
                                            .iter()
                                            .position(|channel| channel.id == channel_id)
                                        {
                                            new_state.chat.selected_channel = idx;
                                            *tui_selected_for_events.write() = Some(channel_id.to_string());
                                            {
                                                let mut guard = selected_channel_binding_for_events.write();
                                                let previous = guard.clone();
                                                *guard = channels.get(idx).map(|channel| {
                                                    SelectedChannelBinding::merged_from_channel(
                                                        channel,
                                                        previous.as_ref(),
                                                    )
                                                });
                                            }
                                        }
                                    }
                                    DispatchCommand::JoinChannel { channel_name } => {
                                        new_state.router.go_to(Screen::Chat);
                                        (cb.chat.on_join_channel)(channel_name, None);
                                    }
                                    DispatchCommand::AcceptPendingHomeInvitation => {
                                        let Some(update_tx) = update_tx_for_events.clone() else {
                                            new_state.toast_error(
                                                "UI update sender is unavailable",
                                            );
                                            continue;
                                        };
                                        let operation =
                                            WorkflowHandoffOperationOwner::submit(
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
                                                new_state.toast_error(
                                                    "UI update sender is unavailable",
                                                );
                                                continue;
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
                                            .filter(|channel_id| {
                                                channels.is_empty()
                                                    || channels
                                                        .iter()
                                                        .any(|channel| channel.id == *channel_id)
                                            });
                                        let visible_message_channel_id =
                                            shared_messages_for_dispatch
                                                .read()
                                                .last()
                                                .map(|message| message.channel_id.clone())
                                                .filter(|channel_id| {
                                                    channels.is_empty()
                                                        || channels
                                                            .iter()
                                                            .any(|channel| channel.id == *channel_id)
                                                });
                                        if let Some(channel_id) = committed_channel_id.or_else(|| {
                                            resolve_committed_selected_channel_id(
                                                &new_state,
                                                &channels,
                                            )
                                        }).or(visible_message_channel_id) {
                                            (cb.chat.on_send)(channel_id, content, operation);
                                        } else {
                                            new_state.toast_error(format!(
                                                "No committed channel selected (channels={} selected_index={} visible_messages={})",
                                                channels.len(),
                                                new_state.chat.selected_channel,
                                                shared_messages_for_dispatch.read().len()
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
                                    DispatchCommand::OpenChatTopicModal => {
                                        let idx = new_state.chat.selected_channel;
                                        let channels = shared_channels_for_dispatch.read().clone();
                                        if let Some(channel) = channels.get(idx) {
                                            let modal_state = crate::tui::state::TopicModalState::for_channel(
                                                &channel.id,
                                                channel.topic.as_deref().unwrap_or(""),
                                            );
                                            new_state
                                                .modal_queue
                                                .enqueue(crate::tui::state::QueuedModal::ChatTopic(
                                                    modal_state,
                                                ));
                                        } else {
                                            new_state.toast_error("No channel selected");
                                        }
                                    }
                                    DispatchCommand::OpenChatInfoModal => {
                                        let idx = new_state.chat.selected_channel;
                                        let channels = shared_channels_for_dispatch.read().clone();
                                        if let Some(channel) = channels.get(idx) {
                                            let mut modal_state = crate::tui::state::ChannelInfoModalState::for_channel(
                                                &channel.id,
                                                &channel.name,
                                                channel.topic.as_deref(),
                                            );

                                            // Best-effort: start with self; authoritative list arrives via list_participants.
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
                                                .enqueue(crate::tui::state::QueuedModal::ChatInfo(
                                                    modal_state,
                                                ));
                                            (cb.chat.on_list_participants)(channel.id.clone());
                                        } else {
                                            new_state.toast_error("No channel selected");
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
                                            continue;
                                        }

                                        let mut candidates: Vec<crate::tui::state::ChatMemberCandidate> =
                                            current_contacts
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
                                        let is_demo_mode = candidates.iter().any(|candidate| {
                                            candidate.id == demo_alice_id
                                                || candidate.id == demo_carol_id
                                        });
                                        let demo_name_rank = |contact_id: &str, name: &str| -> u8 {
                                            if !is_demo_mode {
                                                return 2;
                                            }
                                            if name.eq_ignore_ascii_case("Alice")
                                                || demo_alice_id == contact_id
                                            {
                                                0
                                            } else if name.eq_ignore_ascii_case("Carol")
                                                || demo_carol_id == contact_id
                                            {
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

                                        let mut modal_state =
                                            crate::tui::state::CreateChannelModalState::new();
                                        modal_state.contacts = candidates;
                                        modal_state.ensure_threshold();

                                        new_state.modal_queue.enqueue(
                                            crate::tui::state::QueuedModal::ChatCreate(
                                                modal_state,
                                            ),
                                        );
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
                                                    if let Ok(parsed_id) =
                                                        expected_id.parse::<aura_core::AuthorityId>()
                                                    {
                                                        demo_members.push(parsed_id);
                                                    }
                                                }
                                            }
                                            // Fallback if deterministic IDs are unavailable in the contact list.
                                            for needle in ["Alice", "Carol"] {
                                                if demo_members.len() >= 2 {
                                                    break;
                                                }
                                                if let Some(contact_id) =
                                                    contacts.iter().find_map(|contact| {
                                                        let nickname = contact.nickname.trim();
                                                        let suggested = contact
                                                            .nickname_suggestion
                                                            .as_deref()
                                                            .unwrap_or("")
                                                            .trim();
                                                        if nickname.eq_ignore_ascii_case(needle)
                                                            || suggested.eq_ignore_ascii_case(needle)
                                                        {
                                                            Some(contact.id.clone())
                                                        } else {
                                                            None
                                                        }
                                                    })
                                                {
                                                    if let Ok(parsed_id) =
                                                        contact_id.parse::<aura_core::AuthorityId>()
                                                    {
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
                                            new_state.toast_error(
                                                "UI update sender is unavailable",
                                            );
                                            continue;
                                        };
                                        let operation =
                                            LocalTerminalOperationOwner::submit(
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
                                            Some(operation),
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
                                        (cb.contacts.on_update_nickname)(
                                            contact_id.to_string(),
                                            nickname,
                                        );
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
                                                ).with_suggestion(contact.nickname_suggestion.clone());
                                                new_state
                                                    .modal_queue
                                                    .enqueue(crate::tui::state::QueuedModal::ContactsNickname(
                                                        modal_state,
                                                    ));
                                            } else {
                                                new_state.toast_error("No contact selected");
                                            }
                                        }
                                    }
                                    DispatchCommand::OpenCreateInvitationModal => {
                                        let idx = new_state.contacts.selected_index;
                                        {
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
                                                .enqueue(crate::tui::state::QueuedModal::ContactsCreate(
                                                    modal_state,
                                                ));
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
                                                    new_state.toast_error(
                                                        "Selected peer has no LAN address",
                                                    );
                                                } else {
                                                    (cb.contacts.on_invite_lan_peer)(
                                                        authority_id,
                                                        address,
                                                    );
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
                                            continue;
                                        };
                                        let Some(channel) = channels.get(channel_idx) else {
                                            new_state.toast_error("No channel selected");
                                            continue;
                                        };
                                        let selected_binding = selected_channel_binding_for_events
                                            .read()
                                            .clone()
                                            .filter(|binding| binding.channel_id == channel.id);
                                        let context_id = selected_binding
                                            .and_then(|binding| binding.context_id)
                                            .or_else(|| channel.context_id.clone())
                                            ;
                                        let Some(context_id) = context_id else {
                                            new_state.toast_error(format!(
                                                "Selected channel lacks authoritative context: {}",
                                                channel.id
                                            ));
                                            continue;
                                        };
                                        new_state.clear_runtime_fact_kind(
                                            RuntimeEventKind::PendingHomeInvitationReady,
                                        );
                                        (cb.contacts.on_invite_to_channel)(
                                            contact.id.clone(),
                                            channel.id.clone(),
                                            Some(context_id),
                                            None,
                                        );
                                    }
                                    DispatchCommand::InviteActorToChannel {
                                        authority_id,
                                        channel_id,
                                    } => {
                                        let channels = shared_channels_for_dispatch.read().clone();
                                        let channel_id_string = channel_id.clone();
                                        let Some(channel) =
                                            channels.iter().find(|channel| channel.id == channel_id_string)
                                        else {
                                            new_state.toast_error(format!(
                                                "Selected channel is stale or unavailable: {channel_id}"
                                            ));
                                            continue;
                                        };
                                        let selected_binding = selected_channel_binding_for_events
                                            .read()
                                            .clone()
                                            .filter(|binding| binding.channel_id == channel.id);
                                        let context_id = selected_binding
                                            .and_then(|binding| binding.context_id)
                                            .or_else(|| channel.context_id.clone())
                                            ;
                                        let Some(context_id) = context_id else {
                                            new_state.toast_error(format!(
                                                "Selected channel lacks authoritative context: {}",
                                                channel.id
                                            ));
                                            continue;
                                        };
                                        new_state.clear_runtime_fact_kind(
                                            RuntimeEventKind::PendingHomeInvitationReady,
                                        );
                                        (cb.contacts.on_invite_to_channel)(
                                            authority_id.to_string(),
                                            channel.id.clone(),
                                            Some(context_id),
                                            None,
                                        );
                                    }
                                    DispatchCommand::RemoveContact { contact_id } => {
                                        (cb.contacts.on_remove_contact)(contact_id.to_string());
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
                                                new_state.modal_queue.enqueue(
                                                    crate::tui::state::QueuedModal::Confirm {
                                                        title: "Remove Contact".to_string(),
                                                        message: format!(
                                                            "Are you sure you want to remove \"{display_name}\"?"
                                                        ),
                                                        on_confirm: Some(
                                                            crate::tui::state::ConfirmAction::RemoveContact {
                                                                contact_id: contact.id.clone().into(),
                                                            },
                                                        ),
                                                    },
                                                );
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
                                        if let Some(NotificationSelection::ReceivedInvitation(
                                            invitation_id,
                                        )) = selected
                                        {
                                            (cb.invitations.on_accept)(invitation_id);
                                        } else {
                                            new_state.toast_error(
                                                "Select a received invitation to accept",
                                            );
                                        }
                                    }
                                    DispatchCommand::DeclineInvitation => {
                                        let selected = read_selected_notification(
                                            new_state.notifications.selected_index,
                                            &shared_invitations_for_dispatch,
                                            &shared_pending_requests_for_dispatch,
                                        );
                                        if let Some(NotificationSelection::ReceivedInvitation(
                                            invitation_id,
                                        )) = selected
                                        {
                                            (cb.invitations.on_decline)(invitation_id);
                                        } else {
                                            new_state.toast_error(
                                                "Select a received invitation to decline",
                                            );
                                        }
                                    }
                                    DispatchCommand::CreateInvitation {
                                        receiver_id,
                                        invitation_type,
                                        message,
                                        ttl_secs,
                                    } => {
                                        new_state.clear_runtime_fact_kind(
                                            RuntimeEventKind::InvitationCodeReady,
                                        );
                                        (cb.invitations.on_create)(
                                            receiver_id,
                                            invitation_type.as_str().to_owned(),
                                            message,
                                            ttl_secs,
                                            None,
                                        );
                                    }
                                    DispatchCommand::ImportInvitation { code } => {
                                        let Some(update_tx) = update_tx_for_events.clone() else {
                                            new_state.toast_error(
                                                "UI update sender is unavailable",
                                            );
                                            continue;
                                        };
                                        let operation =
                                            WorkflowHandoffOperationOwner::submit(
                                                app_core_for_events.clone(),
                                                tasks_for_events.clone(),
                                                update_tx,
                                                OperationId::invitation_accept(),
                                                SemanticOperationKind::AcceptContactInvitation,
                                            );
                                        new_state.clear_runtime_fact_kind(
                                            RuntimeEventKind::ContactLinkReady,
                                        );
                                        (cb.invitations.on_import)(code, operation);
                                    }
                                    DispatchCommand::ExportInvitation => {
                                        let selected = read_selected_notification(
                                            new_state.notifications.selected_index,
                                            &shared_invitations_for_dispatch,
                                            &shared_pending_requests_for_dispatch,
                                        );
                                        if let Some(NotificationSelection::SentInvitation(
                                            invitation_id,
                                        )) = selected
                                        {
                                            (cb.invitations.on_export)(invitation_id);
                                        } else {
                                            new_state.toast_error(
                                                "Select a sent invitation to export",
                                            );
                                        }
                                    }
                                    DispatchCommand::RevokeInvitation { invitation_id } => {
                                        (cb.invitations.on_revoke)(invitation_id.to_string());
                                    }

                                    // === Recovery Commands ===
                                    DispatchCommand::StartRecovery => {
                                        // Check recovery eligibility before starting
                                        let (threshold_k, _threshold_n) = *shared_threshold_for_dispatch.read();

                                        // Check if threshold is configured
                                        if threshold_k == 0 {
                                            new_state.toast_error(
                                                RecoveryError::NoThresholdConfigured.to_string(),
                                            );
                                            continue;
                                        }

                                        // Check if we have enough guardians
                                        let guardian_count = shared_contacts_for_dispatch
                                            .read()
                                            .iter()
                                            .filter(|c| c.is_guardian)
                                            .count();

                                        if guardian_count < threshold_k as usize {
                                            new_state.toast_error(
                                                RecoveryError::InsufficientGuardians {
                                                    required: threshold_k,
                                                    available: guardian_count,
                                                }
                                                .to_string(),
                                            );
                                            continue;
                                        }

                                        (cb.recovery.on_start_recovery)();
                                    }
                                    DispatchCommand::ApproveRecovery => {
                                        if let Some(NotificationSelection::RecoveryRequest(req_id)) =
                                            read_selected_notification(
                                                new_state.notifications.selected_index,
                                                &shared_invitations_for_dispatch,
                                                &shared_pending_requests_for_dispatch,
                                            )
                                        {
                                            (cb.recovery.on_submit_approval)(req_id);
                                        } else {
                                            let guard = shared_pending_requests_for_dispatch.read();
                                            if let Some(req) = guard.first() {
                                                (cb.recovery.on_submit_approval)(req.id.clone());
                                            } else {
                                                new_state.toast_error("No pending recovery requests");
                                            }
                                        }
                                    }

                                    // === Guardian Setup Modal ===
                                    DispatchCommand::OpenGuardianSetup => {
                                        // Read current contacts from reactive subscription
                                        // This reads from SharedContacts Arc which is kept up-to-date
                                        // by a separate reactive subscription (not stale props)
                                        let current_contacts = shared_contacts_for_dispatch
                                            .read()
                                            .clone();

                                        // Validate using type-safe ceremony error
                                        if current_contacts.is_empty() {
                                            new_state.toast_error(
                                                GuardianSetupError::NoContacts.to_string(),
                                            );
                                            continue;
                                        }

                                        // Populate candidates from current contacts
                                        // Note: nickname is already populated with nickname_suggestion if empty (see Contact::from)
                                        let candidates: Vec<crate::tui::state::GuardianCandidate> = current_contacts
                                            .iter()
                                            .map(|c| crate::tui::state::GuardianCandidate {
                                                id: c.id.clone(),
                                                name: c.nickname.clone(),
                                                is_current_guardian: c.is_guardian,
                                            })
                                            .collect();

                                        // Pre-select existing guardians
                                        let selected: Vec<usize> = candidates
                                            .iter()
                                            .enumerate()
                                            .filter(|(_, c)| c.is_current_guardian)
                                            .map(|(i, _)| i)
                                            .collect();

                                        // Create populated modal state using factory
                                        let modal_state = crate::tui::state::GuardianSetupModalState::from_contacts_with_selection(candidates, selected);

                                        // Enqueue the modal to new_state (not tui_state, which gets overwritten)
                                        new_state.modal_queue.enqueue(crate::tui::state::QueuedModal::GuardianSetup(modal_state));
                                    }

                                    DispatchCommand::OpenMfaSetup => {
                                        let current_devices = shared_devices_for_dispatch
                                            .read()
                                            .clone();

                                        // Validate using type-safe ceremony error
                                        if current_devices.len() < MIN_MFA_DEVICES {
                                            new_state.toast_error(
                                                MfaSetupError::InsufficientDevices {
                                                    required: MIN_MFA_DEVICES,
                                                    available: current_devices.len(),
                                                }
                                                .to_string(),
                                            );
                                            continue;
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

                                        // Create modal state for MFA setup (pre-selects all, sets threshold)
                                        let modal_state =
                                            crate::tui::state::GuardianSetupModalState::for_mfa_setup(candidates, threshold_k);

                                        new_state.modal_queue.enqueue(
                                            crate::tui::state::QueuedModal::MfaSetup(modal_state),
                                        );
                                    }

                                    // === Guardian Ceremony Commands ===
                                    DispatchCommand::StartGuardianCeremony { contact_ids, threshold_k } => {
                                        tracing::info!(
                                            "Starting guardian ceremony with {} contacts, threshold {}",
                                            contact_ids.len(),
                                            threshold_k.get()
                                        );

                                        let ids = contact_ids.clone();
                                        let n = contact_ids.len() as u16;
                                        let k_raw = threshold_k.get() as u16;

                                        // Create FrostThreshold with validation (FROST requires k >= 2)
                                        let threshold = match FrostThreshold::new(k_raw) {
                                            Ok(t) => t,
                                            Err(e) => {
                                                tracing::error!("Invalid threshold for guardian ceremony: {}", e);
                                                let update_tx = update_tx_for_ceremony.clone();
                                                let tasks = tasks_for_events.clone();
                                                tasks.spawn(async move {
                                                    send_optional_ui_update_required(
                                                        &update_tx,
                                                        UiUpdate::operation_failed(
                                                            UiOperation::StartGuardianCeremony,
                                                            TerminalError::Input(e.to_string()),
                                                        ),
                                                    )
                                                    .await;
                                                });
                                                continue;
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

                                                        // Prime the modal with an initial status update so `ceremony_id` is
                                                        // available immediately for UI cancel.
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

                                                    // Spawn a task to monitor ceremony progress.
                                                    let app_core_monitor = app.clone();
                                                    let update_tx_monitor = update_tx.clone();
                                                    let tasks = tasks.clone();
                                                    let tasks_handle = tasks;
                                                    tasks_handle.spawn(async move {
                                                        let _ = monitor_key_rotation_ceremony(
                                                            &app_core_monitor,
                                                            &status_handle,
                                                            tokio::time::Duration::from_millis(500),
                                                            |status| {
                                                                if let Some(tx) = update_tx_monitor.clone() {
                                                                    let _ = tx.try_send(UiUpdate::KeyRotationCeremonyStatus {
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
                                                                    });
                                                                }
                                                            },
                                                            effect_sleep,
                                                        )
                                                        .await;
                                                    });
                                                }
                                                Err(e) => {
                                                    tracing::error!(
                                                        "Failed to initiate guardian ceremony: {}",
                                                        e
                                                    );

                                                    if let Some(tx) = update_tx {
                                                        send_optional_ui_update_required(
                                                            &Some(tx),
                                                            UiUpdate::operation_failed(
                                                                UiOperation::StartGuardianCeremony,
                                                                TerminalError::Operation(
                                                                    e.to_string(),
                                                                ),
                                                            ),
                                                        )
                                                        .await;
                                                    }
                                                }
                                            }
                                        });
                                    }

                                    DispatchCommand::StartMfaCeremony { device_ids, threshold_k } => {
                                        tracing::info!(
                                            "Starting multifactor ceremony with {} devices, threshold {}",
                                            device_ids.len(),
                                            threshold_k.get()
                                        );

                                        let ids = device_ids.clone();
                                        let n = device_ids.len() as u16;
                                        let k_raw = threshold_k.get() as u16;

                                        let threshold = match FrostThreshold::new(k_raw) {
                                            Ok(t) => t,
                                            Err(e) => {
                                                tracing::error!("Invalid threshold for multifactor ceremony: {}", e);
                                                let update_tx = update_tx_for_ceremony.clone();
                                                let tasks = tasks_for_events.clone();
                                                tasks.spawn(async move {
                                                    send_optional_ui_update_required(
                                                        &update_tx,
                                                        UiUpdate::operation_failed(
                                                            UiOperation::StartMultifactorCeremony,
                                                            TerminalError::Input(e.to_string()),
                                                        ),
                                                    )
                                                    .await;
                                                });
                                                continue;
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
                                                            format!(
                                                                "Multifactor ceremony started ({k}-of-{n})"
                                                            ),
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
                                                        let _ = monitor_key_rotation_ceremony(
                                                            &app_core_monitor,
                                                            &status_handle,
                                                            tokio::time::Duration::from_millis(500),
                                                            |status| {
                                                                if let Some(tx) = update_tx_monitor.clone() {
                                                                    let _ = tx.try_send(UiUpdate::KeyRotationCeremonyStatus {
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
                                                                    });
                                                                }
                                                            },
                                                            effect_sleep,
                                                        )
                                                        .await;
                                                    });
                                                }
                                                Err(e) => {
                                                    tracing::error!(
                                                        "Failed to initiate multifactor ceremony: {}",
                                                        e
                                                    );

                                                    if let Some(tx) = update_tx {
                                                        send_optional_ui_update_required(
                                                            &Some(tx),
                                                            UiUpdate::operation_failed(
                                                                UiOperation::StartMultifactorCeremony,
                                                                TerminalError::Operation(
                                                                    e.to_string(),
                                                                ),
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
                                                Err(e) => {
                                                    tracing::error!("Failed to resolve guardian ceremony handle: {}", e);
                                                    send_optional_ui_update_required(
                                                        &update_tx,
                                                        UiUpdate::operation_failed(
                                                            UiOperation::CancelGuardianCeremony,
                                                            TerminalError::Operation(e.to_string()),
                                                        ),
                                                    )
                                                    .await;
                                                    return;
                                                }
                                            };
                                            if let Err(e) = aura_app::ui::workflows::ceremonies::cancel_key_rotation_ceremony(app, handle).await {
                                                tracing::error!("Failed to cancel guardian ceremony: {}", e);
                                                send_optional_ui_update_required(
                                                    &update_tx,
                                                    UiUpdate::operation_failed(
                                                        UiOperation::CancelGuardianCeremony,
                                                        TerminalError::Operation(e.to_string()),
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
                                                Err(e) => {
                                                    tracing::error!("Failed to resolve ceremony handle: {}", e);
                                                    send_optional_ui_update_required(
                                                        &update_tx,
                                                        UiUpdate::operation_failed(
                                                            UiOperation::CancelKeyRotationCeremony,
                                                            TerminalError::Operation(e.to_string()),
                                                        ),
                                                    )
                                                    .await;
                                                    return;
                                                }
                                            };
                                            if let Err(e) = aura_app::ui::workflows::ceremonies::cancel_key_rotation_ceremony(app, handle).await {
                                                tracing::error!("Failed to cancel ceremony: {}", e);
                                                send_optional_ui_update_required(
                                                    &update_tx,
                                                    UiUpdate::operation_failed(
                                                        UiOperation::CancelKeyRotationCeremony,
                                                        TerminalError::Operation(e.to_string()),
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

                                    // === Settings Screen Commands ===
                                    DispatchCommand::UpdateNicknameSuggestion { nickname_suggestion } => {
                                        (cb.settings.on_update_nickname_suggestion)(nickname_suggestion);
                                    }
                                    DispatchCommand::UpdateMfaPolicy { policy } => {
                                        (cb.settings.on_update_mfa)(policy);
                                    }
                                    DispatchCommand::AddDevice { name, invitee_authority_id } => {
                                        let Some(update_tx) = update_tx_for_events.clone() else {
                                            new_state.toast_error(
                                                "UI update sender is unavailable",
                                            );
                                            continue;
                                        };
                                        let operation = LocalTerminalOperationOwner::submit(
                                            app_core_for_events.clone(),
                                            tasks_for_events.clone(),
                                            update_tx,
                                            OperationId::device_enrollment(),
                                            SemanticOperationKind::StartDeviceEnrollment,
                                        );
                                        (cb.settings.on_add_device)(
                                            name,
                                            invitee_authority_id,
                                            operation,
                                        );
                                    }
                                    DispatchCommand::RemoveDevice { device_id } => {
                                        (cb.settings.on_remove_device)(device_id);
                                    }
                                    DispatchCommand::OpenDeviceSelectModal => {
                                        let current_devices = shared_devices_for_dispatch
                                            .read()
                                            .clone();

                                        if current_devices.is_empty() {
                                            new_state.toast_info("No devices to remove");
                                            continue;
                                        }

                                        // Check if there are any non-current devices
                                        let has_removable = current_devices.iter().any(|d| !d.is_current);
                                        if !has_removable {
                                            new_state.toast_info("Cannot remove the current device");
                                            continue;
                                        }

                                        // Convert to Device type for the modal
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
                                        new_state.modal_queue.enqueue(
                                            crate::tui::state::QueuedModal::SettingsDeviceSelect(modal_state),
                                        );
                                    }
                                    DispatchCommand::ImportDeviceEnrollmentOnMobile { code } => {
                                        let Some(update_tx) = update_tx_for_events.clone() else {
                                            new_state.toast_error(
                                                "UI update sender is unavailable",
                                            );
                                            continue;
                                        };
                                        let operation = LocalTerminalOperationOwner::submit(
                                            app_core_for_events.clone(),
                                            tasks_for_events.clone(),
                                            update_tx,
                                            OperationId::device_enrollment(),
                                            SemanticOperationKind::ImportDeviceEnrollmentCode,
                                        );
                                        (cb.settings.on_import_device_enrollment_on_mobile)(
                                            code,
                                            operation,
                                        );
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
                                                .map(|a| (a.id.clone().into(), format!("{} ({})", a.nickname_suggestion, a.short_id)))
                                                .collect();

                                            let modal_state = crate::tui::state::ContactSelectModalState::single(
                                                "Select Authority",
                                                contacts,
                                            );
                                            new_state.modal_queue.enqueue(
                                                crate::tui::state::QueuedModal::AuthorityPicker(modal_state),
                                            );
                                        }
                                    }
                                    DispatchCommand::SwitchAuthority { authority_id } => {
                                        let authority_id_str = authority_id.to_string();
                                        if let Some(idx) = new_state.authorities
                                            .iter()
                                            .position(|a| a.id == authority_id_str)
                                        {
                                            let nickname = new_state
                                                .authorities
                                                .get(idx)
                                                .and_then(|auth| {
                                                    if auth.nickname_suggestion.trim().is_empty() {
                                                        None
                                                    } else {
                                                        Some(auth.nickname_suggestion.clone())
                                                    }
                                                });
                                            new_state.current_authority_index = idx;
                                            app_ctx_for_dispatch.request_authority_switch(
                                                authority_id,
                                                nickname.clone(),
                                            );
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
                                        new_state.modal_queue.enqueue(
                                            crate::tui::state::QueuedModal::NeighborhoodHomeCreate(
                                                crate::tui::state::HomeCreateModalState::new(),
                                            ),
                                        );
                                    }
                                    DispatchCommand::OpenModeratorAssignmentModal => {
                                        let contacts = shared_contacts_for_dispatch
                                            .read()
                                            .clone();
                                        new_state.modal_queue.enqueue(
                                            crate::tui::state::QueuedModal::NeighborhoodModeratorAssignment(
                                                crate::tui::state::ModeratorAssignmentModalState::new(
                                                    contacts,
                                                ),
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
                                        let contacts = shared_contacts_for_dispatch
                                            .read()
                                            .clone();
                                        new_state.modal_queue.enqueue(
                                            crate::tui::state::QueuedModal::NeighborhoodAccessOverride(
                                                crate::tui::state::AccessOverrideModalState::new(
                                                    contacts,
                                                ),
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
                                                        let _ = tx.try_send(UiUpdate::ToastAdded(
                                                            ToastMessage::success(
                                                                "access-override",
                                                                format!(
                                                                    "Access override set for {}: {}",
                                                                    target_for_toast,
                                                                    access_level.label()
                                                                ),
                                                            ),
                                                        ));
                                                    }
                                                }
                                                Err(error) => {
                                                    if let Some(tx) = update_tx {
                                                        let _ = tx.try_send(UiUpdate::ToastAdded(
                                                            ToastMessage::error(
                                                                "access-override",
                                                                format!(
                                                                    "Failed to set access override: {error}"
                                                                ),
                                                            ),
                                                        ));
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
                                                        let _ = tx.try_send(UiUpdate::ToastAdded(
                                                            ToastMessage::success(
                                                                "capability-config",
                                                                "Capability config saved",
                                                            ),
                                                        ));
                                                    }
                                                }
                                                Err(error) => {
                                                    if let Some(tx) = update_tx {
                                                        let _ = tx.try_send(UiUpdate::ToastAdded(
                                                            ToastMessage::error(
                                                                "capability-config",
                                                                format!(
                                                                    "Failed to save capability config: {error}"
                                                                ),
                                                            ),
                                                        ));
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
                                                (cb.neighborhood.on_add_home_to_neighborhood)(
                                                    home_id.clone(),
                                                );
                                            } else {
                                                new_state.toast_error("No home selected");
                                            }
                                        }
                                    }
                                    DispatchCommand::AddHomeToNeighborhood { target } => {
                                        (cb.neighborhood.on_add_home_to_neighborhood)(
                                            target.as_command_arg(),
                                        );
                                    }
                                    DispatchCommand::LinkSelectedHomeOneHopLink => {
                                        let idx = new_state.neighborhood.grid.current();
                                        {
                                            let guard = shared_neighborhood_homes_for_dispatch.read();
                                            if let Some(home_id) = guard.get(idx) {
                                                (cb.neighborhood.on_link_home_one_hop_link)(
                                                    home_id.clone(),
                                                );
                                            } else {
                                                new_state.toast_error("No home selected");
                                            }
                                        }
                                    }
                                    DispatchCommand::LinkHomeOneHopLink { target } => {
                                        (cb.neighborhood.on_link_home_one_hop_link)(
                                            target.as_command_arg(),
                                        );
                                    }

                                    // === Navigation Commands ===
                                    DispatchCommand::NavigateTo(_screen) => {
                                        // Navigation is handled by TuiState directly
                                        // The state machine already updates the screen
                                    }
                                }
                            }
                            TuiCommand::ShowToast { message, level } => {
                                // Apply UI-only effects to the next state (which is what we persist).
                                let toast_id = new_state.next_toast_id;
                                new_state.next_toast_id += 1;
                                let toast = crate::tui::state::QueuedToast::new(
                                    toast_id,
                                    message,
                                    level,
                                );
                                new_state.toast_queue.enqueue(toast);
                            }
                            TuiCommand::DismissToast { id: _ } => {
                                // Dismiss current toast from queue (ignores ID - FIFO semantics)
                                new_state.toast_queue.dismiss();
                            }
                            TuiCommand::ClearAllToasts => {
                                // Clear all toasts from queue
                                new_state.toast_queue.clear();
                            }
                            TuiCommand::Render => {
                                // Render is handled by iocraft automatically
                            }
                        }
                    }
                }

                // Sync final TuiState changes to iocraft hooks.
                // Important: dispatch commands above can mutate `new_state.router` and
                // `new_state.should_exit`, so synchronization must happen after command execution.
                if new_state.screen() != screen.get() {
                    screen.set(new_state.screen());
                }
                if new_state.should_exit && !should_exit.get() {
                    should_exit.set(true);
                    bg_shutdown.read().store(true, std::sync::atomic::Ordering::Release);
                }

                // Update TuiState (and always bump render version)
                tui.replace(new_state);
            }

            // All key events are handled by the state machine above.
            // Modal handling goes through transition() -> command execution.
        }
    });

    // Nav bar status is updated reactively from signals.
    let network_status = nav_signals.network_status.get();
    let now_ms = nav_signals.now_ms.get();
    let transport_peers = nav_signals.transport_peers.get();
    let known_online = nav_signals.known_online.get();

    // Layout: NavBar (3 rows) + Content (25 rows) + Footer (3 rows) = 31 = TOTAL_HEIGHT
    //
    // Content always renders. Modals overlay via ModalFrame (Position::Absolute).
    // ModalFrame positions at top: NAV_HEIGHT to overlay the content area.

    element! {
        View(
            flex_direction: FlexDirection::Column,
            width: dim::TOTAL_WIDTH,
            height: dim::TOTAL_HEIGHT,
            overflow: Overflow::Hidden,
        ) {
            // Nav bar area (2 rows) - tabs + border
            NavBar(
                active_screen: current_screen,
            )

            // Middle content area (26 rows) - always renders screen content
            // Modals overlay via ModalFrame (absolute positioning)
            View(
                width: dim::TOTAL_WIDTH,
                height: dim::MIDDLE_HEIGHT,
                overflow: Overflow::Hidden,
            ) {
                #(match current_screen {
                    Screen::Chat => vec![element! {
                        View(width: 100pct, height: 100pct) {
                            ChatScreen(
                                view: chat_props.clone(),
                                selected_channel: Some(tui_selected_for_chat_screen),
                                shared_channels: Some(shared_channels),
                                shared_messages: Some(shared_messages),
                                on_send: on_send.clone(),
                                on_retry_message: on_retry_message.clone(),
                                on_create_channel: on_create_channel.clone(),
                                on_set_topic: on_set_topic.clone(),
                                update_tx: update_tx_holder.clone(),
                            )
                        }
                    }],
                    Screen::Contacts => vec![element! {
                        View(width: 100pct, height: 100pct) {
                            ContactsScreen(
                                view: contacts_props.clone(),
                                now_ms: now_ms,
                                on_update_nickname: on_update_nickname.clone(),
                                on_start_chat: on_start_chat.clone(),
                                on_invite_lan_peer: on_invite_lan_peer.clone(),
                            )
                        }
                    }],
                    Screen::Neighborhood => vec![element! {
                        View(width: 100pct, height: 100pct) {
                            NeighborhoodScreen(
                                view: neighborhood_props.clone(),
                                update_tx: update_tx_holder.clone(),
                            )
                        }
                    }],
                    Screen::Settings => vec![element! {
                        View(width: 100pct, height: 100pct) {
                            SettingsScreen(
                                view: settings_props.clone(),
                                on_update_mfa: on_update_mfa.clone(),
                                on_update_nickname_suggestion: on_update_nickname_suggestion.clone(),
                                on_update_threshold: on_update_threshold.clone(),
                                on_add_device: on_add_device.clone(),
                                on_remove_device: on_remove_device.clone(),
                            )
                        }
                    }],
                    Screen::Notifications => vec![element! {
                        View(width: 100pct, height: 100pct) {
                            NotificationsScreen(
                                view: notifications_props.clone(),
                            )
                        }
                    }],
                })
            }

            // Footer with key hints and status (3 rows)
            Footer(
                hints: screen_hints.clone(),
                global_hints: global_hints.clone(),
                disabled: is_insert_mode,
                network_status: network_status.clone(),
                now_ms: now_ms,
                transport_peers: transport_peers,
                known_online: known_online,
                state_indicator: Some(state_indicator),
            )

            // === GLOBAL MODALS ===
            #(render_account_setup_modal(&global_modals))
            #(render_guardian_modal(&global_modals))
            #(render_contact_modal(&global_modals))
            #(render_confirm_modal(&global_modals))
            #(render_help_modal(&global_modals))

            // === SCREEN-SPECIFIC MODALS ===
            // Rendered via modal_overlays module for maintainability
            #(render_nickname_modal(&contacts_props))
            #(render_contacts_import_modal(&contacts_props))
            #(render_contacts_create_modal(&contacts_props))
            #(render_contacts_code_modal(&contacts_props))
            #(render_guardian_setup_modal(&contacts_props))

            // === CHAT SCREEN MODALS ===
            // Rendered via modal_overlays module for maintainability
            #(render_chat_create_modal(&chat_props))
            #(render_topic_modal(&chat_props))
            #(render_channel_info_modal(&chat_props))

            // === SETTINGS SCREEN MODALS ===
            // Rendered via modal_overlays module for maintainability
            // Note: Threshold changes now use OpenGuardianSetup (see contacts screen modals)
            #(render_nickname_suggestion_modal(&settings_props))
            #(render_add_device_modal(&settings_props))
            #(render_device_import_modal(&settings_props))
            #(render_device_enrollment_modal(&settings_props))
            #(render_device_select_modal(&settings_props))
            #(render_remove_device_modal(&settings_props))
            #(render_mfa_setup_modal(&settings_props))

            // === NEIGHBORHOOD SCREEN MODALS ===
            // Rendered via modal_overlays module for maintainability
            #(render_home_create_modal(&neighborhood_props))
            #(render_moderator_assignment_modal(&neighborhood_props))
            #(render_access_override_modal(&neighborhood_props))
            #(render_capability_config_modal(&neighborhood_props))

            // === TOAST OVERLAY ===
            // Toast notifications overlay the footer when active
            // All toasts now go through the queue system (type-enforced single toast at a time)
            #(if let Some(ref toast) = queued_toast {
                Some(element! {
                    ToastContainer(toasts: vec![ToastMessage {
                        id: toast.id.to_string(),
                        message: toast.message.clone(),
                        level: match toast.level {
                            crate::tui::state::ToastLevel::Info => ToastLevel::Info,
                            crate::tui::state::ToastLevel::Success => ToastLevel::Success,
                            crate::tui::state::ToastLevel::Warning => ToastLevel::Warning,
                            crate::tui::state::ToastLevel::Error => ToastLevel::Error,
                        },
                    }])
                })
            } else {
                None
            })
        }
    }
}

/// Run the application with IoContext (real data)
///
/// This version uses the IoContext to fetch actual data from the reactive
/// views instead of mock data.
pub async fn run_app_with_context(ctx: IoContext) -> std::io::Result<()> {
    // Create the UI update channel for reactive updates
    let (update_tx, update_rx) = ui_update_channel();
    let update_rx_holder = Arc::new(Mutex::new(Some(update_rx)));
    let (harness_command_tx, harness_command_rx) = harness_command_channel();
    let harness_command_rx_holder = Arc::new(Mutex::new(Some(harness_command_rx)));
    ensure_harness_command_listener()?;
    register_harness_command_sender(harness_command_tx);

    // Create effect dispatch callbacks using CallbackRegistry
    let ctx_arc = Arc::new(ctx);
    let app_core = ctx_arc.app_core_raw().clone();
    let callbacks = CallbackRegistry::new(ctx_arc.clone(), update_tx.clone(), app_core);

    // Create CallbackContext for providing callbacks to components via iocraft context
    let callback_context = CallbackContext::new(callbacks.clone());

    // Check if account already exists to determine if we show setup modal
    let show_account_setup = !ctx_arc.has_account();

    // ========================================================================
    // Reactive Pattern: All data is provided via signals, not polling.
    // Props below are intentionally empty seeds that are overwritten on mount.
    // ========================================================================
    // Screens subscribe to their respective signals and update reactively:
    // - ChatScreen subscribes to CHAT_SIGNAL
    // - NotificationsScreen subscribes to INVITATIONS_SIGNAL + RECOVERY_SIGNAL
    // - ContactsScreen subscribes to CONTACTS_SIGNAL + DISCOVERED_PEERS_SIGNAL
    // - NeighborhoodScreen subscribes to NEIGHBORHOOD_SIGNAL + HOMES_SIGNAL + CHAT_SIGNAL + CONTACTS_SIGNAL
    // - SettingsScreen subscribes to SETTINGS_SIGNAL (+ RECOVERY_SIGNAL for recovery data)
    //
    // Props passed below are ONLY used as empty/default initial values.
    // Screens ignore these and use signal data immediately on mount.

    let channels = Vec::new();
    let messages = Vec::new();
    let guardians = Vec::new();
    let invitations = Vec::new();
    let contacts = Vec::new();
    let discovered_peers: Vec<DiscoveredPeerInfo> = Vec::new();

    // Neighborhood data - reactively updated via signals
    let neighborhood_name = String::from("Neighborhood");
    let homes: Vec<HomeSummary> = Vec::new();

    // Settings data - reactively updated via SETTINGS_SIGNAL
    let devices = Vec::new();
    let nickname_suggestion = {
        let reactive = {
            let core = ctx_arc.app_core_raw().read().await;
            core.reactive().clone()
        };
        reactive
            .read(&*SETTINGS_SIGNAL)
            .await
            .unwrap_or_default()
            .nickname_suggestion
    };
    let threshold_k = 0;
    let threshold_n = 0;

    // Status bar values are updated reactively after mount.
    // Avoid blocking before entering fullscreen (important for demo mode).
    let network_status = NetworkStatus::Disconnected;
    let transport_peers: usize = 0;
    let known_online: usize = 0;

    // Create AppCoreContext for components to access AppCore and signals
    // AppCore is always available (demo mode uses agent-less AppCore)
    let app_core_context = AppCoreContext::new(ctx_arc.app_core().clone(), ctx_arc.clone());
    // Wrap the app in nested ContextProviders
    // This enables components to use:
    // - `hooks.use_context::<AppCoreContext>()` for reactive signal subscription
    // - `hooks.use_context::<CallbackContext>()` for accessing domain callbacks
    {
        let app_context = app_core_context;
        let cb_context = callback_context;
        #[cfg(feature = "development")]
        let mut app = element! {
            ContextProvider(value: Context::owned(app_context)) {
                ContextProvider(value: Context::owned(cb_context)) {
                    IoApp(
                        // Chat screen data
                        channels: channels,
                        messages: messages,
                        // Invitations data
                        invitations: invitations,
                        guardians: guardians,
                        // Settings screen data
                        devices: devices,
                        nickname_suggestion: nickname_suggestion,
                        threshold_k: threshold_k,
                        threshold_n: threshold_n,
                        mfa_policy: MfaPolicy::SensitiveOnly,
                        // Contacts screen data
                        contacts: contacts,
                        discovered_peers: discovered_peers,
                        // Neighborhood screen data
                        neighborhood_name: neighborhood_name,
                        homes: homes,
                        access_level: AccessLevel::Limited,
                        // Account setup
                        show_account_setup: show_account_setup,
                        pending_runtime_bootstrap: ctx_arc.pending_runtime_bootstrap(),
                        // Network status
                        network_status: network_status.clone(),
                        transport_peers: transport_peers,
                        known_online: known_online,
                        // Demo mode (get from context)
                        demo_mode: ctx_arc.is_demo_mode(),
                        demo_alice_code: ctx_arc.demo_alice_code(),
                        demo_carol_code: ctx_arc.demo_carol_code(),
                        demo_mobile_device_id: ctx_arc.demo_mobile_device_id(),
                        demo_mobile_authority_id: ctx_arc.demo_mobile_authority_id(),
                        // Reactive update channel
                        update_rx: Some(update_rx_holder),
                        harness_command_rx: Some(harness_command_rx_holder),
                        update_tx: Some(update_tx.clone()),
                        // Callbacks registry
                        callbacks: Some(callbacks),
                    )
                }
            }
        };

        #[cfg(not(feature = "development"))]
        let mut app = element! {
            ContextProvider(value: Context::owned(app_context)) {
                ContextProvider(value: Context::owned(cb_context)) {
                    IoApp(
                        // Chat screen data
                        channels: channels,
                        messages: messages,
                        // Invitations data
                        invitations: invitations,
                        guardians: guardians,
                        // Settings screen data
                        devices: devices,
                        nickname_suggestion: nickname_suggestion,
                        threshold_k: threshold_k,
                        threshold_n: threshold_n,
                        mfa_policy: MfaPolicy::SensitiveOnly,
                        // Contacts screen data
                        contacts: contacts,
                        discovered_peers: discovered_peers,
                        // Neighborhood screen data
                        neighborhood_name: neighborhood_name,
                        homes: homes,
                        access_level: AccessLevel::Limited,
                        // Account setup
                        show_account_setup: show_account_setup,
                        pending_runtime_bootstrap: ctx_arc.pending_runtime_bootstrap(),
                        // Network status
                        network_status: network_status,
                        transport_peers: transport_peers,
                        known_online: known_online,
                        // Reactive update channel
                        update_rx: Some(update_rx_holder),
                        harness_command_rx: Some(harness_command_rx_holder),
                        update_tx: Some(update_tx.clone()),
                        // Callbacks registry
                        callbacks: Some(callbacks),
                    )
                }
            }
        };

        let result = app.fullscreen().await;
        clear_harness_command_sender();
        result
    }
}
