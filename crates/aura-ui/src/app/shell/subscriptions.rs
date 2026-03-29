use super::*;
use std::{cell::RefCell, future::Future, rc::Rc};

#[derive(Default)]
struct CoalescedRefreshState {
    loading: bool,
    dirty: bool,
}

fn set_signal_if_changed<T>(
    mut signal: Signal<T>,
    next: T,
    controller: &UiController,
) where
    T: Clone + PartialEq + 'static,
{
    if signal() == next {
        return;
    }
    signal.set(next);
    controller.request_rerender();
}

fn schedule_coalesced_runtime_refresh<T, Loader, Fut>(
    owner: &crate::task_owner::FrontendTaskOwner,
    controller: Arc<UiController>,
    signal: Signal<T>,
    refresh_state: Rc<RefCell<CoalescedRefreshState>>,
    loader: Loader,
) where
    T: Clone + PartialEq + 'static,
    Loader: Fn(Arc<UiController>) -> Fut + Clone + 'static,
    Fut: Future<Output = T> + 'static,
{
    let should_spawn = {
        let mut state = refresh_state.borrow_mut();
        if state.loading {
            state.dirty = true;
            false
        } else {
            state.loading = true;
            true
        }
    };

    if !should_spawn {
        return;
    }

    owner.spawn_local_cancellable(async move {
        loop {
            let next = loader(controller.clone()).await;
            set_signal_if_changed(signal, next, controller.as_ref());
            let rerun = {
                let mut state = refresh_state.borrow_mut();
                if state.dirty {
                    state.dirty = false;
                    true
                } else {
                    state.loading = false;
                    false
                }
            };
            if !rerun {
                break;
            }
        }
    });
}

pub(in crate::app) fn use_runtime_bridge_subscriptions(
    controller: Arc<UiController>,
    mut runtime_bridge_started: Signal<bool>,
    neighborhood_runtime: Signal<NeighborhoodRuntimeView>,
    chat_runtime: Signal<ChatRuntimeView>,
    contacts_runtime: Signal<ContactsRuntimeView>,
    settings_runtime: Signal<SettingsRuntimeView>,
    notifications_runtime: Signal<NotificationsRuntimeView>,
) {
    let subscription_task_owner = use_hook(crate::task_owner::new_ui_task_owner);
    let neighborhood_refresh_state =
        use_hook(|| Rc::new(RefCell::new(CoalescedRefreshState::default())));
    let chat_refresh_state =
        use_hook(|| Rc::new(RefCell::new(CoalescedRefreshState::default())));
    let contacts_refresh_state =
        use_hook(|| Rc::new(RefCell::new(CoalescedRefreshState::default())));
    let settings_refresh_state =
        use_hook(|| Rc::new(RefCell::new(CoalescedRefreshState::default())));
    let notifications_refresh_state =
        use_hook(|| Rc::new(RefCell::new(CoalescedRefreshState::default())));
    let controller_for_runtime = controller;
    use_effect(move || {
        if runtime_bridge_started() {
            return;
        }

        runtime_bridge_started.set(true);

        let runtime_for_initial = neighborhood_runtime;
        let controller_for_initial = controller_for_runtime.clone();
        schedule_coalesced_runtime_refresh(
            &subscription_task_owner,
            controller_for_initial,
            runtime_for_initial,
            neighborhood_refresh_state.clone(),
            load_neighborhood_runtime_view,
        );

        let chat_for_initial = chat_runtime;
        let controller_for_chat_initial = controller_for_runtime.clone();
        schedule_coalesced_runtime_refresh(
            &subscription_task_owner,
            controller_for_chat_initial,
            chat_for_initial,
            chat_refresh_state.clone(),
            load_chat_runtime_view,
        );

        let contacts_for_initial = contacts_runtime;
        let controller_for_contacts_initial = controller_for_runtime.clone();
        schedule_coalesced_runtime_refresh(
            &subscription_task_owner,
            controller_for_contacts_initial,
            contacts_for_initial,
            contacts_refresh_state.clone(),
            load_contacts_runtime_view,
        );

        let settings_for_initial = settings_runtime;
        let controller_for_settings_initial = controller_for_runtime.clone();
        schedule_coalesced_runtime_refresh(
            &subscription_task_owner,
            controller_for_settings_initial,
            settings_for_initial,
            settings_refresh_state.clone(),
            load_settings_runtime_view,
        );

        let notifications_for_initial = notifications_runtime;
        let controller_for_notifications_initial = controller_for_runtime.clone();
        schedule_coalesced_runtime_refresh(
            &subscription_task_owner,
            controller_for_notifications_initial,
            notifications_for_initial,
            notifications_refresh_state.clone(),
            load_notifications_runtime_view,
        );

        let runtime_for_neighborhood = neighborhood_runtime;
        let controller_for_neighborhood = controller_for_runtime.clone();
        let subscription_task_owner_for_neighborhood = subscription_task_owner.clone();
        let neighborhood_refresh_state_for_neighborhood = neighborhood_refresh_state.clone();
        subscription_task_owner.spawn_local_cancellable(async move {
            let Ok(mut stream) = ({
                let core = controller_for_neighborhood.app_core().read().await;
                core.subscribe(&*NEIGHBORHOOD_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                schedule_coalesced_runtime_refresh(
                    &subscription_task_owner_for_neighborhood,
                    controller_for_neighborhood.clone(),
                    runtime_for_neighborhood,
                    neighborhood_refresh_state_for_neighborhood.clone(),
                    load_neighborhood_runtime_view,
                );
            }
        });

        let runtime_for_homes = neighborhood_runtime;
        let controller_for_homes = controller_for_runtime.clone();
        let subscription_task_owner_for_homes = subscription_task_owner.clone();
        let neighborhood_refresh_state_for_homes = neighborhood_refresh_state.clone();
        subscription_task_owner.spawn_local_cancellable(async move {
            let Ok(mut stream) = ({
                let core = controller_for_homes.app_core().read().await;
                core.subscribe(&*HOMES_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                schedule_coalesced_runtime_refresh(
                    &subscription_task_owner_for_homes,
                    controller_for_homes.clone(),
                    runtime_for_homes,
                    neighborhood_refresh_state_for_homes.clone(),
                    load_neighborhood_runtime_view,
                );
            }
        });

        let runtime_for_contacts = neighborhood_runtime;
        let controller_for_contacts = controller_for_runtime.clone();
        let subscription_task_owner_for_runtime_contacts = subscription_task_owner.clone();
        let neighborhood_refresh_state_for_runtime_contacts =
            neighborhood_refresh_state.clone();
        subscription_task_owner.spawn_local_cancellable(async move {
            let Ok(mut stream) = ({
                let core = controller_for_contacts.app_core().read().await;
                core.subscribe(&*CONTACTS_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                schedule_coalesced_runtime_refresh(
                    &subscription_task_owner_for_runtime_contacts,
                    controller_for_contacts.clone(),
                    runtime_for_contacts,
                    neighborhood_refresh_state_for_runtime_contacts.clone(),
                    load_neighborhood_runtime_view,
                );
            }
        });

        let contacts_for_contacts_signal = contacts_runtime;
        let controller_for_contacts_signal = controller_for_runtime.clone();
        let subscription_task_owner_for_contacts_signal = subscription_task_owner.clone();
        let contacts_refresh_state_for_contacts_signal = contacts_refresh_state.clone();
        subscription_task_owner.spawn_local_cancellable(async move {
            let Ok(mut stream) = ({
                let core = controller_for_contacts_signal.app_core().read().await;
                core.subscribe(&*CONTACTS_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                schedule_coalesced_runtime_refresh(
                    &subscription_task_owner_for_contacts_signal,
                    controller_for_contacts_signal.clone(),
                    contacts_for_contacts_signal,
                    contacts_refresh_state_for_contacts_signal.clone(),
                    load_contacts_runtime_view,
                );
            }
        });

        let contacts_for_discovered_peers = contacts_runtime;
        let controller_for_discovered_peers = controller_for_runtime.clone();
        let subscription_task_owner_for_discovered_peers = subscription_task_owner.clone();
        let contacts_refresh_state_for_discovered_peers = contacts_refresh_state.clone();
        subscription_task_owner.spawn_local_cancellable(async move {
            let Ok(mut stream) = ({
                let core = controller_for_discovered_peers.app_core().read().await;
                core.subscribe(&*DISCOVERED_PEERS_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                schedule_coalesced_runtime_refresh(
                    &subscription_task_owner_for_discovered_peers,
                    controller_for_discovered_peers.clone(),
                    contacts_for_discovered_peers,
                    contacts_refresh_state_for_discovered_peers.clone(),
                    load_contacts_runtime_view,
                );
            }
        });

        let runtime_for_chat = neighborhood_runtime;
        let controller_for_chat = controller_for_runtime.clone();
        let subscription_task_owner_for_runtime_chat = subscription_task_owner.clone();
        let neighborhood_refresh_state_for_runtime_chat = neighborhood_refresh_state.clone();
        subscription_task_owner.spawn_local_cancellable(async move {
            let Ok(mut stream) = ({
                let core = controller_for_chat.app_core().read().await;
                core.subscribe(&*CHAT_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                schedule_coalesced_runtime_refresh(
                    &subscription_task_owner_for_runtime_chat,
                    controller_for_chat.clone(),
                    runtime_for_chat,
                    neighborhood_refresh_state_for_runtime_chat.clone(),
                    load_neighborhood_runtime_view,
                );
            }
        });

        let chat_for_chat_signal = chat_runtime;
        let controller_for_chat_signal = controller_for_runtime.clone();
        let subscription_task_owner_for_chat_signal = subscription_task_owner.clone();
        let chat_refresh_state_for_chat_signal = chat_refresh_state.clone();
        subscription_task_owner.spawn_local_cancellable(async move {
            let Ok(mut stream) = ({
                let core = controller_for_chat_signal.app_core().read().await;
                core.subscribe(&*CHAT_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                schedule_coalesced_runtime_refresh(
                    &subscription_task_owner_for_chat_signal,
                    controller_for_chat_signal.clone(),
                    chat_for_chat_signal,
                    chat_refresh_state_for_chat_signal.clone(),
                    load_chat_runtime_view,
                );
            }
        });

        let runtime_for_network = neighborhood_runtime;
        let controller_for_network = controller_for_runtime.clone();
        let subscription_task_owner_for_network = subscription_task_owner.clone();
        let neighborhood_refresh_state_for_network = neighborhood_refresh_state.clone();
        subscription_task_owner.spawn_local_cancellable(async move {
            let Ok(mut stream) = ({
                let core = controller_for_network.app_core().read().await;
                core.subscribe(&*NETWORK_STATUS_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                schedule_coalesced_runtime_refresh(
                    &subscription_task_owner_for_network,
                    controller_for_network.clone(),
                    runtime_for_network,
                    neighborhood_refresh_state_for_network.clone(),
                    load_neighborhood_runtime_view,
                );
            }
        });

        let runtime_for_transport = neighborhood_runtime;
        let controller_for_transport = controller_for_runtime.clone();
        let subscription_task_owner_for_transport = subscription_task_owner.clone();
        let neighborhood_refresh_state_for_transport = neighborhood_refresh_state.clone();
        subscription_task_owner.spawn_local_cancellable(async move {
            let Ok(mut stream) = ({
                let core = controller_for_transport.app_core().read().await;
                core.subscribe(&*TRANSPORT_PEERS_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                schedule_coalesced_runtime_refresh(
                    &subscription_task_owner_for_transport,
                    controller_for_transport.clone(),
                    runtime_for_transport,
                    neighborhood_refresh_state_for_transport.clone(),
                    load_neighborhood_runtime_view,
                );
            }
        });

        let settings_for_settings_signal = settings_runtime;
        let controller_for_settings_signal = controller_for_runtime.clone();
        let subscription_task_owner_for_settings_signal = subscription_task_owner.clone();
        let settings_refresh_state_for_settings_signal = settings_refresh_state.clone();
        subscription_task_owner.spawn_local_cancellable(async move {
            let Ok(mut stream) = ({
                let core = controller_for_settings_signal.app_core().read().await;
                core.subscribe(&*SETTINGS_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                schedule_coalesced_runtime_refresh(
                    &subscription_task_owner_for_settings_signal,
                    controller_for_settings_signal.clone(),
                    settings_for_settings_signal,
                    settings_refresh_state_for_settings_signal.clone(),
                    load_settings_runtime_view,
                );
            }
        });

        let settings_for_recovery_signal = settings_runtime;
        let controller_for_recovery_signal = controller_for_runtime.clone();
        let subscription_task_owner_for_recovery_signal = subscription_task_owner.clone();
        let settings_refresh_state_for_recovery_signal = settings_refresh_state.clone();
        subscription_task_owner.spawn_local_cancellable(async move {
            let Ok(mut stream) = ({
                let core = controller_for_recovery_signal.app_core().read().await;
                core.subscribe(&*RECOVERY_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                schedule_coalesced_runtime_refresh(
                    &subscription_task_owner_for_recovery_signal,
                    controller_for_recovery_signal.clone(),
                    settings_for_recovery_signal,
                    settings_refresh_state_for_recovery_signal.clone(),
                    load_settings_runtime_view,
                );
            }
        });

        let notifications_for_invites = notifications_runtime;
        let controller_for_invites = controller_for_runtime.clone();
        let subscription_task_owner_for_invites = subscription_task_owner.clone();
        let notifications_refresh_state_for_invites = notifications_refresh_state.clone();
        subscription_task_owner.spawn_local_cancellable(async move {
            let Ok(mut stream) = ({
                let core = controller_for_invites.app_core().read().await;
                core.subscribe(&*INVITATIONS_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                schedule_coalesced_runtime_refresh(
                    &subscription_task_owner_for_invites,
                    controller_for_invites.clone(),
                    notifications_for_invites,
                    notifications_refresh_state_for_invites.clone(),
                    load_notifications_runtime_view,
                );
            }
        });

        let notifications_for_recovery = notifications_runtime;
        let controller_for_notifications_recovery = controller_for_runtime.clone();
        let subscription_task_owner_for_notifications_recovery =
            subscription_task_owner.clone();
        let notifications_refresh_state_for_recovery =
            notifications_refresh_state.clone();
        subscription_task_owner.spawn_local_cancellable(async move {
            let Ok(mut stream) = ({
                let core = controller_for_notifications_recovery
                    .app_core()
                    .read()
                    .await;
                core.subscribe(&*RECOVERY_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                schedule_coalesced_runtime_refresh(
                    &subscription_task_owner_for_notifications_recovery,
                    controller_for_notifications_recovery.clone(),
                    notifications_for_recovery,
                    notifications_refresh_state_for_recovery.clone(),
                    load_notifications_runtime_view,
                );
            }
        });

        let notifications_for_errors = notifications_runtime;
        let controller_for_errors = controller_for_runtime.clone();
        let subscription_task_owner_for_errors = subscription_task_owner.clone();
        let notifications_refresh_state_for_errors = notifications_refresh_state.clone();
        subscription_task_owner.spawn_local_cancellable(async move {
            let Ok(mut stream) = ({
                let core = controller_for_errors.app_core().read().await;
                core.subscribe(&*ERROR_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                schedule_coalesced_runtime_refresh(
                    &subscription_task_owner_for_errors,
                    controller_for_errors.clone(),
                    notifications_for_errors,
                    notifications_refresh_state_for_errors.clone(),
                    load_notifications_runtime_view,
                );
            }
        });

        let controller_for_authoritative_operations = controller_for_runtime.clone();
        subscription_task_owner.spawn_local_cancellable(async move {
            let Ok(mut stream) = ({
                let core = controller_for_authoritative_operations
                    .app_core()
                    .read()
                    .await;
                core.subscribe(&*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                let facts = {
                    let core = controller_for_authoritative_operations
                        .app_core()
                        .read()
                        .await;
                    core.read(&*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL)
                        .await
                        .unwrap_or_default()
                };
                for (_kind, fact) in facts.iter().filter_map(|fact| fact.runtime_fact_bridge()) {
                    controller_for_authoritative_operations.push_runtime_fact(fact);
                }
                for (operation_id, _instance_id, _causality, status) in
                    bridged_operation_statuses(&facts)
                {
                    controller_for_authoritative_operations.apply_authoritative_operation_status(
                        operation_id,
                        _instance_id,
                        _causality,
                        status,
                    );
                }
            }
        });
    });
}
