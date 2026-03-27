use super::*;

pub(in crate::app) fn use_runtime_bridge_subscriptions(
    controller: Arc<UiController>,
    mut runtime_bridge_started: Signal<bool>,
    neighborhood_runtime: Signal<NeighborhoodRuntimeView>,
    chat_runtime: Signal<ChatRuntimeView>,
    contacts_runtime: Signal<ContactsRuntimeView>,
    settings_runtime: Signal<SettingsRuntimeView>,
    notifications_runtime: Signal<NotificationsRuntimeView>,
) {
    let controller_for_runtime = controller;
    use_effect(move || {
        if runtime_bridge_started() {
            return;
        }

        runtime_bridge_started.set(true);

        let mut runtime_for_initial = neighborhood_runtime;
        let controller_for_initial = controller_for_runtime.clone();
        spawn_ui(async move {
            runtime_for_initial.set(load_neighborhood_runtime_view(controller_for_initial).await);
        });

        let mut chat_for_initial = chat_runtime;
        let controller_for_chat_initial = controller_for_runtime.clone();
        spawn_ui(async move {
            chat_for_initial.set(load_chat_runtime_view(controller_for_chat_initial.clone()).await);
            controller_for_chat_initial.request_rerender();
        });

        let mut contacts_for_initial = contacts_runtime;
        let controller_for_contacts_initial = controller_for_runtime.clone();
        spawn_ui(async move {
            contacts_for_initial
                .set(load_contacts_runtime_view(controller_for_contacts_initial.clone()).await);
            controller_for_contacts_initial.request_rerender();
        });

        let mut settings_for_initial = settings_runtime;
        let controller_for_settings_initial = controller_for_runtime.clone();
        spawn_ui(async move {
            settings_for_initial
                .set(load_settings_runtime_view(controller_for_settings_initial.clone()).await);
            controller_for_settings_initial.request_rerender();
        });

        let mut notifications_for_initial = notifications_runtime;
        let controller_for_notifications_initial = controller_for_runtime.clone();
        spawn_ui(async move {
            notifications_for_initial.set(
                load_notifications_runtime_view(controller_for_notifications_initial.clone()).await,
            );
            controller_for_notifications_initial.request_rerender();
        });

        let mut runtime_for_neighborhood = neighborhood_runtime;
        let controller_for_neighborhood = controller_for_runtime.clone();
        spawn_ui(async move {
            let Ok(mut stream) = ({
                let core = controller_for_neighborhood.app_core().read().await;
                core.subscribe(&*NEIGHBORHOOD_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                runtime_for_neighborhood
                    .set(load_neighborhood_runtime_view(controller_for_neighborhood.clone()).await);
                controller_for_neighborhood.request_rerender();
            }
        });

        let mut runtime_for_homes = neighborhood_runtime;
        let controller_for_homes = controller_for_runtime.clone();
        spawn_ui(async move {
            let Ok(mut stream) = ({
                let core = controller_for_homes.app_core().read().await;
                core.subscribe(&*HOMES_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                runtime_for_homes
                    .set(load_neighborhood_runtime_view(controller_for_homes.clone()).await);
                controller_for_homes.request_rerender();
            }
        });

        let mut runtime_for_contacts = neighborhood_runtime;
        let controller_for_contacts = controller_for_runtime.clone();
        spawn_ui(async move {
            let Ok(mut stream) = ({
                let core = controller_for_contacts.app_core().read().await;
                core.subscribe(&*CONTACTS_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                runtime_for_contacts
                    .set(load_neighborhood_runtime_view(controller_for_contacts.clone()).await);
                controller_for_contacts.request_rerender();
            }
        });

        let mut contacts_for_contacts_signal = contacts_runtime;
        let controller_for_contacts_signal = controller_for_runtime.clone();
        spawn_ui(async move {
            let Ok(mut stream) = ({
                let core = controller_for_contacts_signal.app_core().read().await;
                core.subscribe(&*CONTACTS_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                contacts_for_contacts_signal
                    .set(load_contacts_runtime_view(controller_for_contacts_signal.clone()).await);
                controller_for_contacts_signal.request_rerender();
            }
        });

        let mut contacts_for_discovered_peers = contacts_runtime;
        let controller_for_discovered_peers = controller_for_runtime.clone();
        spawn_ui(async move {
            let Ok(mut stream) = ({
                let core = controller_for_discovered_peers.app_core().read().await;
                core.subscribe(&*DISCOVERED_PEERS_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                contacts_for_discovered_peers
                    .set(load_contacts_runtime_view(controller_for_discovered_peers.clone()).await);
                controller_for_discovered_peers.request_rerender();
            }
        });

        let mut runtime_for_chat = neighborhood_runtime;
        let controller_for_chat = controller_for_runtime.clone();
        spawn_ui(async move {
            let Ok(mut stream) = ({
                let core = controller_for_chat.app_core().read().await;
                core.subscribe(&*CHAT_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                runtime_for_chat
                    .set(load_neighborhood_runtime_view(controller_for_chat.clone()).await);
                controller_for_chat.request_rerender();
            }
        });

        let mut chat_for_chat_signal = chat_runtime;
        let controller_for_chat_signal = controller_for_runtime.clone();
        spawn_ui(async move {
            let Ok(mut stream) = ({
                let core = controller_for_chat_signal.app_core().read().await;
                core.subscribe(&*CHAT_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                chat_for_chat_signal
                    .set(load_chat_runtime_view(controller_for_chat_signal.clone()).await);
                controller_for_chat_signal.request_rerender();
            }
        });

        let mut runtime_for_network = neighborhood_runtime;
        let controller_for_network = controller_for_runtime.clone();
        spawn_ui(async move {
            let Ok(mut stream) = ({
                let core = controller_for_network.app_core().read().await;
                core.subscribe(&*NETWORK_STATUS_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                runtime_for_network
                    .set(load_neighborhood_runtime_view(controller_for_network.clone()).await);
                controller_for_network.request_rerender();
            }
        });

        let mut runtime_for_transport = neighborhood_runtime;
        let controller_for_transport = controller_for_runtime.clone();
        spawn_ui(async move {
            let Ok(mut stream) = ({
                let core = controller_for_transport.app_core().read().await;
                core.subscribe(&*TRANSPORT_PEERS_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                runtime_for_transport
                    .set(load_neighborhood_runtime_view(controller_for_transport.clone()).await);
                controller_for_transport.request_rerender();
            }
        });

        let mut settings_for_settings_signal = settings_runtime;
        let controller_for_settings_signal = controller_for_runtime.clone();
        spawn_ui(async move {
            let Ok(mut stream) = ({
                let core = controller_for_settings_signal.app_core().read().await;
                core.subscribe(&*SETTINGS_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                settings_for_settings_signal
                    .set(load_settings_runtime_view(controller_for_settings_signal.clone()).await);
                controller_for_settings_signal.request_rerender();
            }
        });

        let mut settings_for_recovery_signal = settings_runtime;
        let controller_for_recovery_signal = controller_for_runtime.clone();
        spawn_ui(async move {
            let Ok(mut stream) = ({
                let core = controller_for_recovery_signal.app_core().read().await;
                core.subscribe(&*RECOVERY_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                settings_for_recovery_signal
                    .set(load_settings_runtime_view(controller_for_recovery_signal.clone()).await);
                controller_for_recovery_signal.request_rerender();
            }
        });

        let mut notifications_for_invites = notifications_runtime;
        let controller_for_invites = controller_for_runtime.clone();
        spawn_ui(async move {
            let Ok(mut stream) = ({
                let core = controller_for_invites.app_core().read().await;
                core.subscribe(&*INVITATIONS_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                notifications_for_invites
                    .set(load_notifications_runtime_view(controller_for_invites.clone()).await);
                controller_for_invites.request_rerender();
            }
        });

        let mut notifications_for_recovery = notifications_runtime;
        let controller_for_notifications_recovery = controller_for_runtime.clone();
        spawn_ui(async move {
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
                notifications_for_recovery.set(
                    load_notifications_runtime_view(controller_for_notifications_recovery.clone())
                        .await,
                );
                controller_for_notifications_recovery.request_rerender();
            }
        });

        let mut notifications_for_errors = notifications_runtime;
        let controller_for_errors = controller_for_runtime.clone();
        spawn_ui(async move {
            let Ok(mut stream) = ({
                let core = controller_for_errors.app_core().read().await;
                core.subscribe(&*ERROR_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                notifications_for_errors
                    .set(load_notifications_runtime_view(controller_for_errors.clone()).await);
                controller_for_errors.request_rerender();
            }
        });

        let controller_for_authoritative_operations = controller_for_runtime.clone();
        spawn_ui(async move {
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
