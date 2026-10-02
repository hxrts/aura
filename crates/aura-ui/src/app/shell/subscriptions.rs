use super::*;
use aura_app::ui_contract::{SubscriptionFailureCode, SubscriptionHealthState};
use aura_core::effects::reactive::{ReactiveError, Signal as ReactiveSignal};
use aura_core::ownership::OwnedTaskSpawner;
use futures::{Stream, StreamExt};
use std::collections::{HashMap, HashSet};
use std::{cell::RefCell, future::Future, rc::Rc};

#[derive(Default)]
struct CoalescedRefreshState {
    loading: bool,
    dirty: bool,
}

// A cancelled loader must release the slot so a later event can schedule a
// fresh snapshot. The component owner cancels all of its loaders on unmount.
struct RefreshSlot(Rc<RefCell<CoalescedRefreshState>>);

impl Drop for RefreshSlot {
    fn drop(&mut self) {
        let mut state = self.0.borrow_mut();
        state.loading = false;
        state.dirty = false;
    }
}

#[derive(Default)]
struct SubscriptionSupervisor {
    failures: u8,
}

impl SubscriptionSupervisor {
    fn observed_update(&mut self) {
        self.failures = 0;
    }

    fn failed(&mut self, reason: SubscriptionFailureCode) -> SubscriptionHealthState {
        self.failures = self.failures.saturating_add(1);
        SubscriptionHealthState::Degraded { reason }
    }

    fn can_retry(&self) -> bool {
        self.failures < 3
    }
}

fn register_unique_signal(registry: &Rc<RefCell<HashSet<String>>>, signal: &str) {
    assert!(
        registry.borrow_mut().insert(signal.to_owned()),
        "duplicate UI subscription for {signal}"
    );
}

fn use_subscription_task_owner() -> crate::task_owner::FrontendTaskOwner {
    use_hook(crate::task_owner::new_ui_task_owner)
}

fn set_signal_if_changed<T>(mut signal: Signal<T>, next: T, controller: &UiController)
where
    T: Clone + PartialEq + 'static,
{
    if signal() != next {
        signal.set(next);
        controller.request_rerender();
    }
}

fn schedule_coalesced_runtime_refresh<T, Loader, Fut>(
    spawner: &OwnedTaskSpawner,
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
    spawner.spawn_local_cancellable(Box::pin(async move {
        let _slot = RefreshSlot(refresh_state.clone());
        loop {
            let next = loader(controller.clone()).await;
            set_signal_if_changed(signal, next, controller.as_ref());
            let rerun = {
                let mut state = refresh_state.borrow_mut();
                if state.dirty {
                    state.dirty = false;
                    true
                } else {
                    false
                }
            };
            if !rerun {
                break;
            }
        }
    }));
}

fn display_guardian_contact_name(contact: &aura_app::ui::types::Contact) -> String {
    if !contact.nickname.trim().is_empty() {
        return contact.nickname.clone();
    }
    if let Some(suggestion) = contact
        .nickname_suggestion
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        return suggestion.clone();
    }
    contact.id.to_string().chars().take(8).collect()
}

fn contact_acceptance_refresh_keys(facts: &[AuthoritativeSemanticFact]) -> HashSet<String> {
    facts
        .iter()
        .filter_map(|fact| match fact {
            AuthoritativeSemanticFact::InvitationAccepted {
                invitation_kind: aura_app::ui_contract::InvitationFactKind::Contact,
                authority_id: Some(_),
                ..
            }
            | AuthoritativeSemanticFact::ContactLinkReady { .. } => Some(fact.key()),
            _ => None,
        })
        .collect()
}

async fn observe_attempt<S, T, F, Fut, H>(
    attachment: Result<S, ReactiveError>,
    supervisor: &mut SubscriptionSupervisor,
    mut on_snapshot: F,
    mut on_health: H,
) -> SubscriptionFailureCode
where
    S: Stream<Item = Result<T, ReactiveError>>,
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<(), SubscriptionFailureCode>>,
    H: FnMut(SubscriptionHealthState),
{
    let stream = match attachment {
        Ok(stream) => stream,
        Err(_) => {
            let reason = SubscriptionFailureCode::RegistrationFailed;
            on_health(supervisor.failed(reason));
            return reason;
        }
    };
    futures::pin_mut!(stream);
    match on_snapshot().await {
        Ok(()) => on_health(SubscriptionHealthState::Healthy),
        Err(reason) => on_health(SubscriptionHealthState::Degraded { reason }),
    }
    let reason = loop {
        match stream.next().await {
            Some(Ok(_)) => {
                supervisor.observed_update();
                match on_snapshot().await {
                    Ok(()) => on_health(SubscriptionHealthState::Healthy),
                    Err(reason) => on_health(SubscriptionHealthState::Degraded { reason }),
                }
            }
            Some(Err(ReactiveError::SubscriptionClosed { .. })) | None => {
                break SubscriptionFailureCode::StreamClosed;
            }
            Some(Err(_)) => break SubscriptionFailureCode::SnapshotReadFailed,
        }
    };
    on_health(supervisor.failed(reason));
    reason
}

// Each signal gets exactly one receiver. The callback fans out to every
// projection that depends on it and runs once immediately after attachment,
// closing the subscribe/read race and resnapshotting after a missed update.
fn supervise_signal<T, F, Fut>(
    spawner: &OwnedTaskSpawner,
    registry: &Rc<RefCell<HashSet<String>>>,
    controller: Arc<UiController>,
    signal: &'static ReactiveSignal<T>,
    on_snapshot: F,
) where
    T: Clone + Send + Sync + 'static,
    F: Fn(Arc<UiController>) -> Fut + 'static,
    Fut: Future<Output = Result<(), SubscriptionFailureCode>> + 'static,
{
    let label = signal.id().to_string();
    register_unique_signal(registry, &label);
    controller.set_subscription_health(&label, SubscriptionHealthState::Attaching);
    spawner.spawn_local_cancellable(Box::pin(async move {
        let mut supervisor = SubscriptionSupervisor::default();
        loop {
            let attachment = {
                let core = controller.app_core().read().await;
                core.subscribe(signal)
            };
            let stream = attachment.map(|stream| {
                futures::stream::unfold(stream, |mut stream| async move {
                    Some((stream.recv().await, stream))
                })
            });
            let failure_reason = observe_attempt(
                stream,
                &mut supervisor,
                || on_snapshot(controller.clone()),
                |state| controller.set_subscription_health(&label, state),
            )
            .await;
            if !supervisor.can_retry() {
                // A new runtime generation remounts the shell and gets a new
                // owner; keep the terminal failure observable until then.
                break;
            }
            if time_workflows::sleep_ms(controller.app_core(), 250)
                .await
                .is_err()
            {
                controller.set_subscription_health(
                    &label,
                    SubscriptionHealthState::Degraded {
                        reason: failure_reason,
                    },
                );
                break;
            }
            controller.set_subscription_health(&label, SubscriptionHealthState::Recovering);
        }
    }));
}

fn refresh_neighborhood(
    spawner: &OwnedTaskSpawner,
    controller: Arc<UiController>,
    view: Signal<NeighborhoodRuntimeView>,
    state: &Rc<RefCell<CoalescedRefreshState>>,
) {
    schedule_coalesced_runtime_refresh(
        spawner,
        controller,
        view,
        state.clone(),
        load_neighborhood_runtime_view,
    );
}

fn refresh_chat(
    spawner: &OwnedTaskSpawner,
    controller: Arc<UiController>,
    view: Signal<ChatRuntimeView>,
    state: &Rc<RefCell<CoalescedRefreshState>>,
) {
    schedule_coalesced_runtime_refresh(
        spawner,
        controller,
        view,
        state.clone(),
        load_chat_runtime_view,
    );
}

fn refresh_contacts(
    spawner: &OwnedTaskSpawner,
    controller: Arc<UiController>,
    view: Signal<ContactsRuntimeView>,
    state: &Rc<RefCell<CoalescedRefreshState>>,
) {
    schedule_coalesced_runtime_refresh(
        spawner,
        controller,
        view,
        state.clone(),
        load_contacts_runtime_view,
    );
}

fn refresh_settings(
    spawner: &OwnedTaskSpawner,
    controller: Arc<UiController>,
    view: Signal<SettingsRuntimeView>,
    state: &Rc<RefCell<CoalescedRefreshState>>,
) {
    schedule_coalesced_runtime_refresh(
        spawner,
        controller,
        view,
        state.clone(),
        load_settings_runtime_view,
    );
}

fn refresh_notifications(
    spawner: &OwnedTaskSpawner,
    controller: Arc<UiController>,
    view: Signal<NotificationsRuntimeView>,
    state: &Rc<RefCell<CoalescedRefreshState>>,
) {
    schedule_coalesced_runtime_refresh(
        spawner,
        controller,
        view,
        state.clone(),
        load_notifications_runtime_view,
    );
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
    let subscription_task_owner = use_subscription_task_owner();
    let neighborhood_state = use_hook(|| Rc::new(RefCell::new(CoalescedRefreshState::default())));
    let chat_state = use_hook(|| Rc::new(RefCell::new(CoalescedRefreshState::default())));
    let contacts_state = use_hook(|| Rc::new(RefCell::new(CoalescedRefreshState::default())));
    let settings_state = use_hook(|| Rc::new(RefCell::new(CoalescedRefreshState::default())));
    let notifications_state = use_hook(|| Rc::new(RefCell::new(CoalescedRefreshState::default())));
    let observed_guardians = use_hook(|| Rc::new(RefCell::new(None::<HashMap<String, String>>)));
    let observed_devices = use_hook(|| Rc::new(RefCell::new(None::<HashMap<String, String>>)));
    let authoritative_contact_keys = use_hook(|| Rc::new(RefCell::new(HashSet::<String>::new())));
    let subscription_registry = use_hook(|| Rc::new(RefCell::new(HashSet::<String>::new())));

    use_effect(move || {
        if runtime_bridge_started() {
            return;
        }
        runtime_bridge_started.set(true);
        // The component is the sole owner. Tasks hold only its spawner, so
        // dropping the component cancels them instead of retaining the owner.
        let spawner = subscription_task_owner.owned_spawner();
        refresh_neighborhood(
            &spawner,
            controller.clone(),
            neighborhood_runtime,
            &neighborhood_state,
        );
        refresh_chat(&spawner, controller.clone(), chat_runtime, &chat_state);
        refresh_contacts(
            &spawner,
            controller.clone(),
            contacts_runtime,
            &contacts_state,
        );
        refresh_settings(
            &spawner,
            controller.clone(),
            settings_runtime,
            &settings_state,
        );
        refresh_notifications(
            &spawner,
            controller.clone(),
            notifications_runtime,
            &notifications_state,
        );

        macro_rules! watch_neighborhood {
            ($signal:expr) => {{
                let task_spawner = spawner.clone();
                let state = neighborhood_state.clone();
                supervise_signal(
                    &spawner,
                    &subscription_registry,
                    controller.clone(),
                    &*$signal,
                    move |controller| {
                        refresh_neighborhood(
                            &task_spawner,
                            controller,
                            neighborhood_runtime,
                            &state,
                        );
                        async { Ok(()) }
                    },
                );
            }};
        }
        watch_neighborhood!(NEIGHBORHOOD_SIGNAL);
        watch_neighborhood!(HOMES_SIGNAL);
        watch_neighborhood!(NETWORK_STATUS_SIGNAL);
        watch_neighborhood!(TRANSPORT_PEERS_SIGNAL);

        {
            let task_spawner = spawner.clone();
            let neighborhood_state = neighborhood_state.clone();
            let contacts_state = contacts_state.clone();
            let notifications_state = notifications_state.clone();
            let observed_guardians = observed_guardians.clone();
            supervise_signal(
                &spawner,
                &subscription_registry,
                controller.clone(),
                &CONTACTS_SIGNAL,
                move |controller| {
                    refresh_neighborhood(
                        &task_spawner,
                        controller.clone(),
                        neighborhood_runtime,
                        &neighborhood_state,
                    );
                    refresh_contacts(
                        &task_spawner,
                        controller.clone(),
                        contacts_runtime,
                        &contacts_state,
                    );
                    refresh_notifications(
                        &task_spawner,
                        controller.clone(),
                        notifications_runtime,
                        &notifications_state,
                    );
                    let observed_guardians = observed_guardians.clone();
                    async move {
                        let contacts = {
                            let core = controller.app_core().read().await;
                            core.read(&CONTACTS_SIGNAL)
                                .await
                                .map_err(|_| SubscriptionFailureCode::SnapshotReadFailed)?
                        };
                        let current = contacts
                            .all_contacts()
                            .filter(|contact| contact.is_guardian)
                            .map(|contact| {
                                (
                                    contact.id.to_string(),
                                    display_guardian_contact_name(contact),
                                )
                            })
                            .collect::<HashMap<_, _>>();
                        let added = {
                            let mut previous = observed_guardians.borrow_mut();
                            let added = previous
                                .as_ref()
                                .map(|known| {
                                    current
                                        .iter()
                                        .filter(|(id, _)| !known.contains_key(*id))
                                        .map(|(id, name)| (id.clone(), name.clone()))
                                        .collect::<Vec<_>>()
                                })
                                .unwrap_or_default();
                            *previous = Some(current);
                            added
                        };
                        for (authority_id, guardian_name) in added {
                            controller.push_runtime_fact(RuntimeFact::GuardianInvitationAccepted {
                                authority_id: Some(authority_id),
                                guardian_name: Some(guardian_name),
                            });
                        }
                        Ok(())
                    }
                },
            );
        }

        {
            let task_spawner = spawner.clone();
            let contacts_state = contacts_state.clone();
            supervise_signal(
                &spawner,
                &subscription_registry,
                controller.clone(),
                &DISCOVERED_PEERS_SIGNAL,
                move |controller| {
                    refresh_contacts(&task_spawner, controller, contacts_runtime, &contacts_state);
                    async { Ok(()) }
                },
            );
        }

        {
            let task_spawner = spawner.clone();
            let neighborhood_state = neighborhood_state.clone();
            let chat_state = chat_state.clone();
            supervise_signal(
                &spawner,
                &subscription_registry,
                controller.clone(),
                &CHAT_SIGNAL,
                move |controller| {
                    refresh_neighborhood(
                        &task_spawner,
                        controller.clone(),
                        neighborhood_runtime,
                        &neighborhood_state,
                    );
                    refresh_chat(&task_spawner, controller, chat_runtime, &chat_state);
                    async { Ok(()) }
                },
            );
        }

        {
            let task_spawner = spawner.clone();
            let settings_state = settings_state.clone();
            let observed_devices = observed_devices.clone();
            supervise_signal(
                &spawner,
                &subscription_registry,
                controller.clone(),
                &SETTINGS_SIGNAL,
                move |controller| {
                    refresh_settings(
                        &task_spawner,
                        controller.clone(),
                        settings_runtime,
                        &settings_state,
                    );
                    let observed_devices = observed_devices.clone();
                    async move {
                        let settings = {
                            let core = controller.app_core().read().await;
                            core.read(&SETTINGS_SIGNAL)
                                .await
                                .map_err(|_| SubscriptionFailureCode::SnapshotReadFailed)?
                        };
                        let current = settings
                            .devices
                            .iter()
                            .map(|device| (device.id.to_string(), device.name.clone()))
                            .collect::<HashMap<_, _>>();
                        let added = {
                            let mut previous = observed_devices.borrow_mut();
                            let added = previous
                                .as_ref()
                                .map(|known| {
                                    current
                                        .iter()
                                        .filter(|(id, _)| !known.contains_key(*id))
                                        .map(|(id, name)| (id.clone(), name.clone()))
                                        .collect::<Vec<_>>()
                                })
                                .unwrap_or_default();
                            *previous = Some(current);
                            added
                        };
                        for (device_id, device_name) in added {
                            controller.push_runtime_fact(RuntimeFact::DeviceEnrollmentAccepted {
                                device_id: Some(device_id),
                                device_name: Some(device_name),
                                device_count: Some(settings.devices.len()),
                            });
                        }
                        Ok(())
                    }
                },
            );
        }

        {
            let task_spawner = spawner.clone();
            let settings_state = settings_state.clone();
            let notifications_state = notifications_state.clone();
            supervise_signal(
                &spawner,
                &subscription_registry,
                controller.clone(),
                &RECOVERY_SIGNAL,
                move |controller| {
                    refresh_settings(
                        &task_spawner,
                        controller.clone(),
                        settings_runtime,
                        &settings_state,
                    );
                    refresh_notifications(
                        &task_spawner,
                        controller,
                        notifications_runtime,
                        &notifications_state,
                    );
                    async { Ok(()) }
                },
            );
        }

        macro_rules! watch_notifications {
            ($signal:expr) => {{
                let task_spawner = spawner.clone();
                let state = notifications_state.clone();
                supervise_signal(
                    &spawner,
                    &subscription_registry,
                    controller.clone(),
                    &*$signal,
                    move |controller| {
                        refresh_notifications(
                            &task_spawner,
                            controller,
                            notifications_runtime,
                            &state,
                        );
                        async { Ok(()) }
                    },
                );
            }};
        }
        watch_notifications!(INVITATIONS_SIGNAL);
        watch_notifications!(ERROR_SIGNAL);

        {
            let task_spawner = spawner.clone();
            let notifications_state = notifications_state.clone();
            let authoritative_contact_keys = authoritative_contact_keys.clone();
            supervise_signal(
                &spawner,
                &subscription_registry,
                controller.clone(),
                &AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL,
                move |controller| {
                    let task_spawner = task_spawner.clone();
                    let notifications_state = notifications_state.clone();
                    let authoritative_contact_keys = authoritative_contact_keys.clone();
                    async move {
                        let facts = {
                            let core = controller.app_core().read().await;
                            core.read(&AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL)
                                .await
                                .map_err(|_| SubscriptionFailureCode::SnapshotReadFailed)?
                        };
                        for (_kind, fact) in
                            facts.iter().filter_map(|fact| fact.runtime_fact_bridge())
                        {
                            controller.push_runtime_fact(fact);
                        }
                        for (operation_id, instance_id, causality, status) in
                            bridged_operation_statuses(&facts)
                        {
                            controller.apply_authoritative_operation_status(
                                operation_id,
                                instance_id,
                                causality,
                                status,
                            );
                        }
                        refresh_notifications(
                            &task_spawner,
                            controller.clone(),
                            notifications_runtime,
                            &notifications_state,
                        );
                        let keys = contact_acceptance_refresh_keys(&facts);
                        let should_refresh = {
                            let mut previous = authoritative_contact_keys.borrow_mut();
                            let changed = keys.iter().any(|key| !previous.contains(key));
                            *previous = keys;
                            changed
                        };
                        if should_refresh {
                            let app_core = controller.app_core().clone();
                            task_spawner.spawn_local_cancellable(Box::pin(async move {
                                let _ = system_workflows::refresh_account(&app_core).await;
                            }));
                        }
                        Ok(())
                    }
                },
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_app::ui_contract::InvitationFactKind;
    use aura_core::ownership::OwnedShutdownToken;

    thread_local! {
        static MOUNT_TOKENS: RefCell<Vec<OwnedShutdownToken>> = const { RefCell::new(Vec::new()) };
    }

    fn subscription_owner_probe() -> Element {
        let owner = use_subscription_task_owner();
        MOUNT_TOKENS.with(|tokens| tokens.borrow_mut().push(owner.shutdown_token().clone()));
        owner.spawn_local_cancellable(async {
            futures::future::pending::<()>().await;
        });
        rsx! { div {} }
    }

    #[test]
    fn repeated_virtual_dom_mounts_cancel_old_subscription_owners() {
        MOUNT_TOKENS.with(|tokens| tokens.borrow_mut().clear());
        let mut previous: Option<OwnedShutdownToken> = None;
        for _generation in 0..3 {
            let mut dom = dioxus::dioxus_core::VirtualDom::new(subscription_owner_probe);
            dom.rebuild_in_place();
            let current = MOUNT_TOKENS.with(|tokens| {
                tokens
                    .borrow()
                    .last()
                    .cloned()
                    .expect("probe records mounted owner")
            });
            assert!(!current.is_cancelled());
            if let Some(previous) = previous {
                assert!(previous.is_cancelled());
            }
            drop(dom);
            assert!(current.is_cancelled());
            previous = Some(current);
        }
    }

    #[test]
    fn cancelled_refresh_releases_its_slot() {
        let state = Rc::new(RefCell::new(CoalescedRefreshState {
            loading: true,
            dirty: true,
        }));
        drop(RefreshSlot(state.clone()));
        assert!(!state.borrow().loading);
        assert!(!state.borrow().dirty);
    }

    #[test]
    fn supervisor_reports_failed_registration_closure_and_recovery() {
        let mut supervisor = SubscriptionSupervisor::default();
        assert_eq!(
            supervisor.failed(SubscriptionFailureCode::RegistrationFailed),
            SubscriptionHealthState::Degraded {
                reason: SubscriptionFailureCode::RegistrationFailed
            }
        );
        assert!(supervisor.can_retry());
        supervisor.observed_update();
        assert_eq!(supervisor.failures, 0);
        assert_eq!(
            supervisor.failed(SubscriptionFailureCode::StreamClosed),
            SubscriptionHealthState::Degraded {
                reason: SubscriptionFailureCode::StreamClosed
            }
        );
        assert!(supervisor.can_retry());
        supervisor.failed(SubscriptionFailureCode::StreamClosed);
        supervisor.failed(SubscriptionFailureCode::StreamClosed);
        assert!(!supervisor.can_retry());
    }

    #[test]
    fn failed_attachment_is_reported_and_a_later_stream_resnapshots() {
        futures::executor::LocalPool::new().run_until(async {
            let mut supervisor = SubscriptionSupervisor::default();
            let health = Rc::new(RefCell::new(Vec::new()));
            let reported = health.clone();
            let failure = observe_attempt(
                Err::<futures::stream::Empty<Result<(), ReactiveError>>, _>(
                    ReactiveError::SignalNotFound {
                        id: "contacts".to_owned(),
                    },
                ),
                &mut supervisor,
                || async { Ok(()) },
                move |state| reported.borrow_mut().push(state),
            )
            .await;
            assert_eq!(failure, SubscriptionFailureCode::RegistrationFailed);
            assert_eq!(
                health.borrow().as_slice(),
                &[SubscriptionHealthState::Degraded {
                    reason: SubscriptionFailureCode::RegistrationFailed
                }]
            );

            let snapshots = Rc::new(RefCell::new(0usize));
            let counted = snapshots.clone();
            let reported = health.clone();
            let failure = observe_attempt(
                Ok(futures::stream::iter([
                    Ok(()),
                    Err(ReactiveError::SubscriptionClosed {
                        id: "contacts".to_owned(),
                    }),
                ])),
                &mut supervisor,
                move || {
                    *counted.borrow_mut() += 1;
                    async { Ok(()) }
                },
                move |state| reported.borrow_mut().push(state),
            )
            .await;
            assert_eq!(failure, SubscriptionFailureCode::StreamClosed);
            assert_eq!(*snapshots.borrow(), 2); // initial state plus the update
            assert_eq!(
                health.borrow().as_slice(),
                &[
                    SubscriptionHealthState::Degraded {
                        reason: SubscriptionFailureCode::RegistrationFailed
                    },
                    SubscriptionHealthState::Healthy,
                    SubscriptionHealthState::Healthy,
                    SubscriptionHealthState::Degraded {
                        reason: SubscriptionFailureCode::StreamClosed
                    },
                ]
            );
        });
    }

    #[test]
    #[should_panic(expected = "duplicate UI subscription")]
    fn duplicate_signal_observer_is_rejected() {
        let registry = Rc::new(RefCell::new(HashSet::new()));
        register_unique_signal(&registry, "contacts");
        register_unique_signal(&registry, "contacts");
    }

    #[test]
    fn contact_acceptance_refresh_keys_ignore_non_contact_facts() {
        let keys = contact_acceptance_refresh_keys(&[
            AuthoritativeSemanticFact::PendingHomeInvitationReady,
            AuthoritativeSemanticFact::InvitationAccepted {
                invitation_kind: InvitationFactKind::Generic,
                authority_id: Some("peer-a".to_string()),
                operation_state: Some(OperationState::Succeeded),
            },
        ]);
        assert!(keys.is_empty());
    }

    #[test]
    fn contact_acceptance_refresh_keys_capture_contact_acceptance_and_link_facts() {
        let keys = contact_acceptance_refresh_keys(&[
            AuthoritativeSemanticFact::InvitationAccepted {
                invitation_kind: InvitationFactKind::Contact,
                authority_id: Some("peer-a".to_string()),
                operation_state: Some(OperationState::Succeeded),
            },
            AuthoritativeSemanticFact::ContactLinkReady {
                authority_id: "peer-a".to_string(),
                contact_count: 1,
            },
        ]);
        assert_eq!(keys.len(), 2);
        assert!(keys.contains("invitation_accepted:Contact:peer-a"));
        assert!(keys.contains("contact_link_ready:peer-a"));
    }
}
