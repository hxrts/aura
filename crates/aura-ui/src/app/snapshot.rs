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

    let selected_home_id = model
        .selected_home_id()
        .filter(|selected_id| {
            neighborhood_runtime
                .homes
                .iter()
                .any(|home| home.id == *selected_id)
        })
        .map(str::to_string)
        .or_else(|| {
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
            delivery_status: None,
        })
        .collect();
    snapshot.quiescence = aura_app::ui_contract::QuiescenceSnapshot::derive(
        snapshot.readiness,
        snapshot.open_modal,
        &snapshot.operations,
    );

    snapshot
}
#[cfg(test)]
mod projection_source_tests {
    use super::*;
    use crate::app::runtime_views::load_neighborhood_runtime_view;
    use crate::MemoryClipboard;
    use aura_app::{
        ui::{
            types::{HomeState, HomesState},
            workflows::context,
        },
        AppConfig, AppCore, ProjectionSlot,
    };
    use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
    use std::sync::{Arc, OnceLock};

    #[test]
    fn add_device_wizard_cannot_publish_synthetic_completion() {
        let mut model = UiModel::new("authority-local".to_string());
        model.active_modal = Some(ActiveModal::AddDevice(crate::model::AddDeviceModalState {
            step: AddDeviceWizardStep::Confirm,
            is_complete: true,
            ..crate::model::AddDeviceModalState::default()
        }));
        let snapshot = runtime_semantic_snapshot(
            &model,
            &NeighborhoodRuntimeView::default(),
            &ChatRuntimeView::default(),
            &ContactsRuntimeView::default(),
            &SettingsRuntimeView::default(),
            &NotificationsRuntimeView::default(),
        );
        assert!(snapshot.operations.is_empty());
    }

    #[test]
    fn published_home_list_carries_its_app_graph_source_revision() {
        futures::executor::LocalPool::new().run_until(async {
            let app_core = Arc::new(async_lock::RwLock::new(
                AppCore::new(AppConfig::default()).expect("app core"),
            ));
            AppCore::init_signals_with_hooks(&app_core)
                .await
                .expect("register app signals");
            let home_id = ChannelId::from_bytes([80u8; 32]);
            let home = HomeState::new(
                home_id,
                Some("Revision home".to_string()),
                AuthorityId::new_from_entropy([81u8; 32]),
                1,
                ContextId::new_from_entropy([82u8; 32]),
            );
            // Build a detached query-style fixture without exposing raw home
            // insertion to production callers.
            let mut serialized = serde_json::to_value(HomesState::new()).unwrap();
            serialized["homes"]
                .as_object_mut()
                .unwrap()
                .insert(home_id.to_string(), serde_json::to_value(home).unwrap());
            let fixture: HomesState = serde_json::from_value(serialized).unwrap();
            let owner = app_core.read().await.projection_owner();
            owner
                .update(ProjectionSlot::homes(), move |homes| {
                    *homes = fixture;
                    Ok::<_, ()>(())
                })
                .await
                .expect("publish home through app projection owner")
                .expect("fixture update is infallible");
            context::add_home_to_neighborhood(&app_core, &home_id.to_string())
                .await
                .expect("materialize the home in the neighborhood projection");
            context::move_position(&app_core, &home_id.to_string(), "full")
                .await
                .expect("mirror the home revision into the app snapshot");
            let graph_revision = app_core
                .read()
                .await
                .projection_owner()
                .snapshot(ProjectionSlot::homes())
                .await
                .expect("homes graph snapshot")
                .revision;
            let app_snapshot = app_core.read().await.snapshot();
            assert_eq!(
                app_snapshot.projection_source_revisions.homes,
                Some(graph_revision)
            );

            let controller = Arc::new(UiController::new(
                app_core,
                Arc::new(MemoryClipboard::default()),
            ));
            let published = Arc::new(OnceLock::new());
            controller.set_ui_snapshot_sink(Arc::new({
                let published = published.clone();
                move |snapshot| {
                    let _ = published.set(snapshot);
                }
            }));
            let neighborhood = load_neighborhood_runtime_view(controller.clone()).await;
            let model = UiModel::new(String::new());
            let snapshot = runtime_semantic_snapshot(
                &model,
                &neighborhood,
                &ChatRuntimeView::default(),
                &ContactsRuntimeView::default(),
                &SettingsRuntimeView::default(),
                &NotificationsRuntimeView::default(),
            );
            controller.publish_ui_snapshot(snapshot);
            let exported = published.get().expect("published snapshot");
            let homes = exported
                .lists
                .iter()
                .find(|list| list.id == ListId::Homes)
                .expect("rendered homes list");
            assert!(homes
                .items
                .iter()
                .any(|item| item.id == home_id.to_string()));
            assert_eq!(
                exported.projection_source_revisions.homes,
                Some(graph_revision)
            );
            assert_eq!(
                controller.ui_snapshot().projection_source_revisions.homes,
                Some(graph_revision)
            );
        });
    }
}
