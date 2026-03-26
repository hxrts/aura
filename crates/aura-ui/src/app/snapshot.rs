use super::*;

fn upsert_snapshot_list(
    snapshot: &mut UiSnapshot,
    list_id: ListId,
    items: Vec<ListItemSnapshot>,
    selected_item_id: Option<String>,
) {
    snapshot.lists.retain(|list| list.id != list_id);
    snapshot
        .selections
        .retain(|selection| selection.list != list_id);
    if items.is_empty() {
        return;
    }
    snapshot.lists.push(ListSnapshot { id: list_id, items });
    if let Some(item_id) = selected_item_id {
        snapshot.selections.push(SelectionSnapshot {
            list: list_id,
            item_id,
        });
    }
}

fn upsert_snapshot_operation(
    snapshot: &mut UiSnapshot,
    operation_id: OperationId,
    state: OperationState,
) {
    snapshot
        .operations
        .retain(|operation| operation.id != operation_id);
    snapshot.operations.push(OperationSnapshot {
        id: operation_id,
        instance_id: OperationInstanceId("synthetic-operation".to_string()),
        state,
    });
}

pub(in crate::app) fn runtime_semantic_snapshot(
    model: &UiModel,
    neighborhood_runtime: &NeighborhoodRuntimeView,
    chat_runtime: &ChatRuntimeView,
    contacts_runtime: &ContactsRuntimeView,
    settings_runtime: &SettingsRuntimeView,
    notifications_runtime: &NotificationsRuntimeView,
) -> UiSnapshot {
    let mut snapshot = model.semantic_snapshot();
    let _ = (
        neighborhood_runtime,
        chat_runtime,
        contacts_runtime,
        settings_runtime,
        notifications_runtime,
    );
    snapshot.readiness = readiness_owner::screen_readiness(
        model.screen,
        readiness_owner::ScreenProjectionReadiness {
            neighborhood_loaded: neighborhood_runtime.loaded,
            neighborhood_home_bound: !neighborhood_runtime.active_home_id.is_empty(),
            chat_loaded: chat_runtime.loaded,
            contacts_loaded: contacts_runtime.loaded,
            settings_loaded: settings_runtime.loaded,
            settings_profile_bound: !settings_runtime.authority_id.is_empty(),
            settings_devices_materialized: !settings_runtime.devices.is_empty(),
            settings_authorities_materialized: !settings_runtime.authorities.is_empty(),
            notifications_loaded: notifications_runtime.loaded,
        },
    );

    if let Some(add_device_state) = model.add_device_modal() {
        let operation_state = match add_device_state.step {
            AddDeviceWizardStep::Name => OperationState::Idle,
            AddDeviceWizardStep::ShareCode | AddDeviceWizardStep::Confirm => {
                if add_device_state.has_failed {
                    OperationState::Failed
                } else if add_device_state.is_complete {
                    OperationState::Succeeded
                } else {
                    OperationState::Submitting
                }
            }
        };
        upsert_snapshot_operation(
            &mut snapshot,
            OperationId::device_enrollment(),
            operation_state,
        );
    }

    let selected_home_id = model.selected_home_id().map(str::to_string).or_else(|| {
        neighborhood_runtime
            .homes
            .iter()
            .find(|home| home.name == neighborhood_runtime.active_home_name)
            .map(|home| home.id.clone())
    });
    let homes = neighborhood_runtime
        .homes
        .iter()
        .map(|home| ListItemSnapshot {
            id: home.id.clone(),
            selected: selected_home_id.as_deref() == Some(home.id.as_str()),
            confirmation: ConfirmationState::Confirmed,
            is_current: false,
        })
        .collect::<Vec<_>>();
    upsert_snapshot_list(&mut snapshot, ListId::Homes, homes, selected_home_id);

    let members = neighborhood_runtime
        .members
        .iter()
        .map(|member| {
            let member_key = neighborhood_member_selection_key(member);
            ListItemSnapshot {
                id: member_key.0.clone(),
                selected: model.selected_neighborhood_member_key.as_ref() == Some(&member_key),
                confirmation: ConfirmationState::Confirmed,
                is_current: false,
            }
        })
        .collect::<Vec<_>>();
    let selected_member_id = model
        .selected_neighborhood_member_key
        .as_ref()
        .map(|key| key.0.clone());
    upsert_snapshot_list(
        &mut snapshot,
        ListId::NeighborhoodMembers,
        members,
        selected_member_id,
    );

    let channels = if chat_runtime.loaded {
        chat_runtime
            .channels
            .iter()
            .map(|channel| ListItemSnapshot {
                id: channel.id.clone(),
                selected: channel
                    .name
                    .eq_ignore_ascii_case(&chat_runtime.active_channel),
                confirmation: ConfirmationState::Confirmed,
                is_current: false,
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let selected_channel_id = if chat_runtime.loaded {
        chat_runtime
            .channels
            .iter()
            .find(|channel| {
                channel
                    .name
                    .eq_ignore_ascii_case(&chat_runtime.active_channel)
            })
            .map(|channel| channel.id.clone())
    } else {
        None
    };
    if !channels.is_empty() {
        upsert_snapshot_list(
            &mut snapshot,
            ListId::Channels,
            channels,
            selected_channel_id,
        );
    }

    let contacts = if contacts_runtime.loaded {
        contacts_runtime
            .contacts
            .iter()
            .map(|contact| ListItemSnapshot {
                id: contact.authority_id.to_string(),
                selected: model.selected_contact_authority_id() == Some(contact.authority_id),
                confirmation: contact.confirmation,
                is_current: false,
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if !contacts.is_empty() {
        upsert_snapshot_list(
            &mut snapshot,
            ListId::Contacts,
            contacts,
            model
                .selected_contact_authority_id()
                .map(|id| id.to_string()),
        );
    }

    let devices = settings_runtime
        .devices
        .iter()
        .map(|device| ListItemSnapshot {
            id: device.id.clone(),
            selected: false,
            confirmation: ConfirmationState::Confirmed,
            is_current: device.is_current,
        })
        .collect::<Vec<_>>();
    upsert_snapshot_list(&mut snapshot, ListId::Devices, devices, None);

    let authorities = settings_runtime
        .authorities
        .iter()
        .map(|authority| ListItemSnapshot {
            id: authority.id.to_string(),
            selected: model.selected_authority_id == Some(authority.id),
            confirmation: ConfirmationState::Confirmed,
            is_current: false,
        })
        .collect::<Vec<_>>();
    if !authorities.is_empty() {
        upsert_snapshot_list(
            &mut snapshot,
            ListId::Authorities,
            authorities,
            model.selected_authority_id.map(|id| id.to_string()),
        );
    }

    let notifications = notifications_runtime
        .items
        .iter()
        .map(|item| ListItemSnapshot {
            id: item.id.clone(),
            selected: model.selected_notification_id.as_ref().map(|id| &id.0) == Some(&item.id),
            confirmation: ConfirmationState::Confirmed,
            is_current: false,
        })
        .collect::<Vec<_>>();
    if !notifications.is_empty() {
        upsert_snapshot_list(
            &mut snapshot,
            ListId::Notifications,
            notifications,
            model
                .selected_notification_id
                .as_ref()
                .map(|id| id.0.clone()),
        );
    }

    snapshot.messages = chat_runtime
        .messages
        .iter()
        .enumerate()
        .map(|(idx, message)| MessageSnapshot {
            id: format!("chat-message-{idx}"),
            content: message.content.clone(),
        })
        .collect();
    snapshot.quiescence = aura_app::ui_contract::QuiescenceSnapshot::derive(
        snapshot.readiness,
        snapshot.open_modal,
        &snapshot.operations,
    );

    snapshot
}
