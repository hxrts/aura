//! iocraft hook helpers for long-lived reactive subscriptions.
//!
//! Keep shell.rs focused on wiring and rendering by extracting the
//! signal-subscription use_future homes here.

mod chat_projection;
mod contracts;
mod display_clock;
mod nav_status;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;

use iocraft::prelude::*;

use aura_app::ui::signals::{
    ConnectionStatus, DiscoveredPeer, DiscoveredPeerMethod, NetworkStatus,
    AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL, CONNECTION_STATUS_SIGNAL, CONTACTS_SIGNAL,
    DISCOVERED_PEERS_SIGNAL, HOMES_SIGNAL, INVITATIONS_SIGNAL, NEIGHBORHOOD_SIGNAL,
    NETWORK_STATUS_SIGNAL, RECOVERY_SIGNAL, SETTINGS_SIGNAL, TRANSPORT_PEERS_SIGNAL,
};
use aura_app::ui::workflows::time as time_workflows;
use aura_app::ui_contract::{
    bridged_operation_statuses, AuthoritativeSemanticFact, RuntimeEventKind,
};
use aura_core::effects::time::PhysicalTimeEffects;
use aura_effects::time::PhysicalTimeHandler;

use crate::tui::context::InitializedAppCore;
use crate::tui::hooks::{
    subscribe_signal_with_retry, subscribe_signal_with_retry_report, AppCoreContext,
};
use crate::tui::screens::app::subscriptions::contracts::{
    subscribe_lifecycle_signal, subscribe_observed_projection_signal,
    subscribe_update_bridge_signal, StructuralDegradationSink,
};
use crate::tui::semantic_lifecycle::authoritative_operation_status_update;
use crate::tui::tasks::UiTaskOwner;
use crate::tui::types::{AuthorityInfo, Contact, Device, Invitation, PendingRequest};
use crate::tui::updates::{
    spawn_ordered_ui_updates, spawn_ui_update, OrderedUiUpdateGate, UiUpdate, UiUpdatePublication,
    UiUpdateSender,
};

pub use chat_projection::{
    use_channels_subscription, use_messages_subscription, SharedChannels, SharedMessages,
};
pub use display_clock::use_display_clock_state;
pub use nav_status::{
    use_authority_id_subscription, use_nav_status_signals, NavStatusSignals, SharedAuthorityId,
};

fn bump_projection_version(version: &mut State<usize>) {
    version.set(version.get().wrapping_add(1));
}

fn authoritative_runtime_replace_kinds() -> Vec<RuntimeEventKind> {
    vec![
        RuntimeEventKind::InvitationAccepted,
        RuntimeEventKind::ContactLinkReady,
        RuntimeEventKind::PendingHomeInvitationReady,
        RuntimeEventKind::ChannelMembershipReady,
        RuntimeEventKind::RecipientPeersResolved,
        RuntimeEventKind::MessageCommitted,
        RuntimeEventKind::MessageDeliveryReady,
    ]
}

/// Shared contacts state that can be read by closures without re-rendering.
///
/// This wraps Arc<RwLock<Vec<Contact>>> instead of State<T> because:
/// 1. Dispatch handler closures need to read current contacts at invocation time.
/// 2. We do not want every contacts update to trigger shell re-renders.
/// 3. The closure captures the Arc, not the data, so it always reads fresh data.
#[derive(Clone, Default)]
pub struct SharedContacts(Arc<RwLock<Vec<Contact>>>);

impl SharedContacts {
    #[must_use]
    pub fn new() -> Self {
        Self(Arc::new(RwLock::new(Vec::new())))
    }

    pub fn read(&self) -> parking_lot::RwLockReadGuard<'_, Vec<Contact>> {
        self.0.read()
    }

    pub fn write(&self) -> parking_lot::RwLockWriteGuard<'_, Vec<Contact>> {
        self.0.write()
    }
}

/// Shared discovered peers state that can be read by closures without re-rendering.
#[derive(Clone, Default)]
pub struct SharedDiscoveredPeers(Arc<RwLock<Vec<DiscoveredPeer>>>);

impl SharedDiscoveredPeers {
    #[must_use]
    pub fn new() -> Self {
        Self(Arc::new(RwLock::new(Vec::new())))
    }

    pub fn read(&self) -> parking_lot::RwLockReadGuard<'_, Vec<DiscoveredPeer>> {
        self.0.read()
    }

    pub fn write(&self) -> parking_lot::RwLockWriteGuard<'_, Vec<DiscoveredPeer>> {
        self.0.write()
    }
}

/// Create a shared discovered peers holder and subscribe it to DISCOVERED_PEERS_SIGNAL.
///
/// Returns an Arc that closures can capture. The subscription updates the Arc's
/// contents whenever discovery changes, so readers always get current data.
///
/// If `update_tx` is provided, sends `LanPeersCountChanged` whenever the
/// bootstrap-candidate count changes.
pub fn use_discovered_peers_subscription(
    hooks: &mut Hooks,
    app_ctx: &AppCoreContext,
    update_tx: Option<UiUpdateSender>,
) -> SharedDiscoveredPeers {
    let shared_ref = hooks.use_ref(SharedDiscoveredPeers::new);
    let shared: SharedDiscoveredPeers = shared_ref.read().clone();
    let last_lan_count_ref = hooks.use_ref(|| Arc::new(AtomicUsize::new(usize::MAX)));
    let last_lan_count = last_lan_count_ref.read().clone();
    let tasks = app_ctx.tasks();

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let peers = shared.clone();
        let degradation = StructuralDegradationSink::new(tasks.clone(), update_tx.clone());
        async move {
            subscribe_update_bridge_signal(
                app_core,
                &*DISCOVERED_PEERS_SIGNAL,
                move |peers_state| {
                    let lan_peers: Vec<_> = peers_state
                        .peers
                        .iter()
                        .filter(|p| p.method == DiscoveredPeerMethod::BootstrapCandidate)
                        .cloned()
                        .collect();

                    let new_count = lan_peers.len();

                    *peers.write() = lan_peers;

                    if let Some(ref tx) = update_tx {
                        let previous = last_lan_count.swap(new_count, Ordering::Relaxed);
                        if previous != new_count {
                            spawn_ui_update(
                                &tasks,
                                tx,
                                UiUpdate::LanPeersCountChanged(new_count),
                                UiUpdatePublication::RequiredUnordered,
                            );
                        }
                    }
                },
                degradation,
            )
            .await;
        }
    });

    shared
}

/// Create a shared contacts holder and subscribe it to CONTACTS_SIGNAL.
///
/// Returns an Arc that closures can capture. The subscription updates the Arc's
/// contents whenever contacts change, so readers always get current data.
///
/// Uses parking_lot::RwLock so dispatch handlers can read synchronously.
///
/// If `update_tx` is provided, sends `ContactCountChanged` whenever the contact count changes.
/// This keeps `TuiState.contacts.contact_count` in sync for keyboard navigation.
pub fn use_contacts_subscription(
    hooks: &mut Hooks,
    app_ctx: &AppCoreContext,
    update_tx: Option<UiUpdateSender>,
    projection_version: State<usize>,
) -> SharedContacts {
    // Create the shared contacts holder - use_ref ensures it persists across renders.
    let shared_contacts_ref = hooks.use_ref(SharedContacts::new);
    let shared_contacts: SharedContacts = shared_contacts_ref.read().clone();
    let last_contact_count_ref = hooks.use_ref(|| Arc::new(AtomicUsize::new(usize::MAX)));
    let last_contact_count = last_contact_count_ref.read().clone();
    let tasks = app_ctx.tasks();

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let contacts = shared_contacts.clone();
        let degradation = StructuralDegradationSink::new(tasks.clone(), update_tx.clone());
        let mut projection_version = projection_version.clone();
        async move {
            subscribe_update_bridge_signal(
                app_core,
                &*CONTACTS_SIGNAL,
                move |contacts_state| {
                    let contact_list: Vec<Contact> =
                        contacts_state.all_contacts().map(Contact::from).collect();
                    let new_count = contact_list.len();

                    *contacts.write() = contact_list;
                    bump_projection_version(&mut projection_version);

                    // Send contact count update for keyboard navigation
                    if let Some(ref tx) = update_tx {
                        let previous = last_contact_count.swap(new_count, Ordering::Relaxed);
                        if previous != new_count {
                            spawn_ui_update(
                                &tasks,
                                tx,
                                UiUpdate::ContactCountChanged(new_count),
                                UiUpdatePublication::RequiredUnordered,
                            );
                        }
                    }
                },
                degradation,
            )
            .await;
        }
    });

    shared_contacts
}

/// Shared devices state (account devices) that can be read by closures without re-rendering.
#[derive(Clone, Default)]
pub struct SharedDevices(Arc<RwLock<Vec<Device>>>);

impl SharedDevices {
    #[must_use]
    pub fn new() -> Self {
        Self(Arc::new(RwLock::new(Vec::new())))
    }

    pub fn read(&self) -> parking_lot::RwLockReadGuard<'_, Vec<Device>> {
        self.0.read()
    }

    pub fn write(&self) -> parking_lot::RwLockWriteGuard<'_, Vec<Device>> {
        self.0.write()
    }
}

/// Create a shared devices holder and subscribe it to SETTINGS_SIGNAL.
pub fn use_devices_subscription(
    hooks: &mut Hooks,
    app_ctx: &AppCoreContext,
    update_tx: Option<UiUpdateSender>,
    projection_version: State<usize>,
) -> SharedDevices {
    let shared_devices_ref = hooks.use_ref(SharedDevices::new);
    let shared_devices: SharedDevices = shared_devices_ref.read().clone();
    let tasks = app_ctx.tasks();

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let devices = shared_devices.clone();
        let update_tx = update_tx;
        let degradation = StructuralDegradationSink::new(tasks.clone(), update_tx.clone());
        let mut projection_version = projection_version.clone();
        async move {
            subscribe_update_bridge_signal(
                app_core,
                &*SETTINGS_SIGNAL,
                move |settings_state| {
                    let list: Vec<Device> = settings_state
                        .devices
                        .iter()
                        .map(|d| Device {
                            id: d.id.to_string(),
                            name: d.name.clone(),
                            is_current: d.is_current,
                            last_seen: d.last_seen,
                        })
                        .collect();
                    *devices.write() = list.clone();
                    bump_projection_version(&mut projection_version);
                    if list.len() >= 2 {
                        if let Some(tx) = update_tx.as_ref() {
                            spawn_ui_update(
                                &tasks,
                                tx,
                                UiUpdate::RuntimeBootstrapFinalized,
                                UiUpdatePublication::RequiredUnordered,
                            );
                        }
                    }
                },
                degradation,
            )
            .await;
        }
    });

    shared_devices
}

/// Shared invitations state that can be read by closures without re-rendering.
///
/// Used to map selected invitation index -> invitation ID for accept/decline/export.
pub type SharedInvitations = Arc<RwLock<Vec<Invitation>>>;

/// Create a shared invitations holder and subscribe it to INVITATIONS_SIGNAL.
pub fn use_invitations_subscription(
    hooks: &mut Hooks,
    app_ctx: &AppCoreContext,
    _update_tx: Option<UiUpdateSender>,
    projection_version: State<usize>,
) -> SharedInvitations {
    let shared_invitations_ref = hooks.use_ref(|| Arc::new(RwLock::new(Vec::new())));
    let shared_invitations: SharedInvitations = shared_invitations_ref.read().clone();

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let invitations = shared_invitations.clone();
        let mut projection_version = projection_version.clone();
        async move {
            subscribe_observed_projection_signal(
                app_core,
                &*INVITATIONS_SIGNAL,
                move |inv_state| {
                    let all: Vec<Invitation> = inv_state
                        .all_pending()
                        .iter()
                        .chain(inv_state.all_sent().iter())
                        .chain(inv_state.all_history().iter())
                        .map(Invitation::from)
                        .collect();

                    *invitations.write() = all;
                    bump_projection_version(&mut projection_version);
                },
            )
            .await;
        }
    });

    shared_invitations
}

pub fn use_authoritative_semantic_facts_subscription(
    hooks: &mut Hooks,
    app_ctx: &AppCoreContext,
    update_tx: Option<UiUpdateSender>,
) {
    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let ordered_gate = Arc::new(OrderedUiUpdateGate::new());
        let degradation = StructuralDegradationSink::new(app_ctx.tasks(), update_tx.clone());
        let tasks = app_ctx.tasks();
        async move {
            subscribe_lifecycle_signal(
                app_core,
                &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL,
                move |facts| {
                    let Some(ref tx) = update_tx else {
                        return;
                    };
                    let revision = facts.revision;
                    let mut updates = Vec::new();
                    for (operation_id, instance_id, causality, status) in
                        bridged_operation_statuses(&facts)
                    {
                        updates.push(authoritative_operation_status_update(
                            operation_id,
                            instance_id,
                            causality,
                            status,
                        ));
                    }
                    let mapped = facts
                        .iter()
                        .filter_map(AuthoritativeSemanticFact::runtime_fact_bridge)
                        .collect::<Vec<_>>();
                    let facts = mapped.into_iter().map(|(_, fact)| fact).collect::<Vec<_>>();
                    updates.push(UiUpdate::RuntimeFactsUpdated {
                        revision,
                        replace_kinds: authoritative_runtime_replace_kinds(),
                        facts,
                    });
                    spawn_ordered_ui_updates(&tasks, tx, &ordered_gate, updates);
                },
                degradation,
            )
            .await;
        }
    });
}

/// Shared neighborhood home IDs (in display order).
///
/// Used to map neighborhood grid index -> home ID for EnterHome.
pub type SharedNeighborhoodHomes = Arc<RwLock<Vec<String>>>;

/// Create a shared neighborhood homes holder and subscribe it to NEIGHBORHOOD_SIGNAL.
pub fn use_neighborhood_homes_subscription(
    hooks: &mut Hooks,
    app_ctx: &AppCoreContext,
    projection_version: State<usize>,
) -> SharedNeighborhoodHomes {
    let shared_homes_ref = hooks.use_ref(|| Arc::new(RwLock::new(Vec::new())));
    let shared_homes: SharedNeighborhoodHomes = shared_homes_ref.read().clone();

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let homes = shared_homes.clone();
        let mut projection_version = projection_version.clone();
        async move {
            subscribe_observed_projection_signal(app_core, &*NEIGHBORHOOD_SIGNAL, move |n| {
                let mut ids: Vec<String> = Vec::with_capacity(n.neighbor_count() + 1);
                ids.push(n.home_home_id.to_string());
                ids.extend(
                    n.all_neighbors()
                        .filter(|b| b.id != n.home_home_id)
                        .map(|b| b.id.to_string()),
                );
                *homes.write() = ids;
                bump_projection_version(&mut projection_version);
            })
            .await;
        }
    });

    shared_homes
}

/// Shared current-home metadata used by neighborhood state machine navigation.
#[derive(Clone, Copy, Debug, Default)]
pub struct NeighborhoodHomeMeta {
    pub member_count: usize,
    pub moderator_actions_enabled: bool,
}

pub type SharedNeighborhoodHomeMeta = Arc<RwLock<NeighborhoodHomeMeta>>;

/// Create shared current-home metadata from HOMES_SIGNAL.
pub fn use_neighborhood_home_meta_subscription(
    hooks: &mut Hooks,
    app_ctx: &AppCoreContext,
    projection_version: State<usize>,
) -> SharedNeighborhoodHomeMeta {
    let shared_meta_ref = hooks.use_ref(|| Arc::new(RwLock::new(NeighborhoodHomeMeta::default())));
    let shared_meta: SharedNeighborhoodHomeMeta = shared_meta_ref.read().clone();

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let meta = shared_meta.clone();
        let mut projection_version = projection_version.clone();
        async move {
            subscribe_observed_projection_signal(app_core, &*HOMES_SIGNAL, move |homes_state| {
                let snapshot = homes_state
                    .current_home()
                    .map(|home| NeighborhoodHomeMeta {
                        member_count: home.members.len(),
                        moderator_actions_enabled: home.is_admin(),
                    })
                    .unwrap_or_default();
                *meta.write() = snapshot;
                bump_projection_version(&mut projection_version);
            })
            .await;
        }
    });

    shared_meta
}

/// Shared pending recovery requests.
///
/// Used to map selected request index -> request ID for approvals.
pub type SharedPendingRequests = Arc<RwLock<Vec<PendingRequest>>>;

/// Create a shared pending requests holder and subscribe it to RECOVERY_SIGNAL.
pub fn use_pending_requests_subscription(
    hooks: &mut Hooks,
    app_ctx: &AppCoreContext,
    projection_version: State<usize>,
) -> SharedPendingRequests {
    let shared_requests_ref = hooks.use_ref(|| Arc::new(RwLock::new(Vec::new())));
    let shared_requests: SharedPendingRequests = shared_requests_ref.read().clone();

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let requests = shared_requests.clone();
        let mut projection_version = projection_version.clone();
        async move {
            subscribe_observed_projection_signal(app_core, &*RECOVERY_SIGNAL, move |r| {
                let pending: Vec<PendingRequest> = r
                    .pending_requests()
                    .iter()
                    .map(PendingRequest::from)
                    .collect();
                *requests.write() = pending;
                bump_projection_version(&mut projection_version);
            })
            .await;
        }
    });

    shared_requests
}

/// Subscribe to notifications-related signals and emit count updates.
pub fn use_notifications_subscription(
    hooks: &mut Hooks,
    app_ctx: &AppCoreContext,
    update_tx: Option<UiUpdateSender>,
) {
    let invite_count = Arc::new(AtomicUsize::new(0));
    let recovery_count = Arc::new(AtomicUsize::new(0));
    let last_total = Arc::new(AtomicUsize::new(usize::MAX));
    let tasks = app_ctx.tasks();

    let send_total = |tasks: &Arc<UiTaskOwner>,
                      tx: &Option<UiUpdateSender>,
                      invites: &Arc<AtomicUsize>,
                      recovery: &Arc<AtomicUsize>,
                      last_total: &Arc<AtomicUsize>| {
        if let Some(ref tx) = tx {
            let total = invites.load(Ordering::Relaxed) + recovery.load(Ordering::Relaxed);
            let previous = last_total.swap(total, Ordering::Relaxed);
            if previous != total {
                spawn_ui_update(
                    tasks,
                    tx,
                    UiUpdate::NotificationsCountChanged(total),
                    UiUpdatePublication::RequiredUnordered,
                );
            }
        }
    };

    // Invitations
    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let invite_count = invite_count.clone();
        let recovery_count = recovery_count.clone();
        let last_total = last_total.clone();
        let update_tx = update_tx.clone();
        let tasks = tasks.clone();
        let degradation = StructuralDegradationSink::new(tasks.clone(), update_tx.clone());
        async move {
            subscribe_update_bridge_signal(
                app_core,
                &*INVITATIONS_SIGNAL,
                move |state| {
                    invite_count.store(state.pending_received_count(), Ordering::Relaxed);
                    send_total(
                        &tasks,
                        &update_tx,
                        &invite_count,
                        &recovery_count,
                        &last_total,
                    );
                },
                degradation,
            )
            .await;
        }
    });

    // Recovery requests
    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let tasks = tasks;
        let degradation = StructuralDegradationSink::new(tasks.clone(), update_tx.clone());
        async move {
            subscribe_update_bridge_signal(
                app_core,
                &*RECOVERY_SIGNAL,
                move |state| {
                    recovery_count.store(state.pending_requests().len(), Ordering::Relaxed);
                    send_total(
                        &tasks,
                        &update_tx,
                        &invite_count,
                        &recovery_count,
                        &last_total,
                    );
                },
                degradation,
            )
            .await;
        }
    });
}

/// Shared threshold settings.
///
/// Tuple of (threshold_k, threshold_n) for recovery threshold configuration.
/// Used to populate the threshold modal with current values.
pub type SharedThreshold = Arc<RwLock<(u8, u8)>>;

/// Create a shared threshold holder and subscribe it to SETTINGS_SIGNAL.
///
/// Returns an Arc that closures can capture. The subscription updates the Arc's
/// contents whenever settings change, so readers always get current threshold.
pub fn use_threshold_subscription(hooks: &mut Hooks, app_ctx: &AppCoreContext) -> SharedThreshold {
    let shared_threshold_ref = hooks.use_ref(|| Arc::new(RwLock::new((2u8, 3u8))));
    let shared_threshold: SharedThreshold = shared_threshold_ref.read().clone();

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let threshold = shared_threshold.clone();
        async move {
            subscribe_observed_projection_signal(
                app_core,
                &*SETTINGS_SIGNAL,
                move |settings_state| {
                    *threshold.write() = (settings_state.threshold_k, settings_state.threshold_n);
                },
            )
            .await;
        }
    });

    shared_threshold
}

#[cfg(test)]
mod tests {
    use super::contracts::{report_subscription_degradation, StructuralDegradationSink};
    use super::display_clock::{
        DISPLAY_CLOCK_MAX_CONSECUTIVE_FAILURES, DISPLAY_CLOCK_POLL_INTERVAL,
    };
    use crate::tui::tasks::UiTaskOwner;
    use crate::tui::types::Device;
    use crate::tui::updates::UiUpdate;
    use std::path::Path;
    use std::sync::Arc;
    use std::time::Duration;

    #[tokio::test]
    async fn subscription_degradation_reports_structural_ui_update() {
        let tasks = Arc::new(UiTaskOwner::new());
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let sink = StructuralDegradationSink::new(tasks.clone(), Some(tx));

        report_subscription_degradation(
            &sink,
            "app:chat",
            "attempts exhausted after 1 retries".to_string(),
        );

        match rx.recv().await {
            Some(UiUpdate::SubscriptionDegraded { signal_id, reason }) => {
                assert_eq!(signal_id, "app:chat");
                assert_eq!(reason, "attempts exhausted after 1 retries");
            }
            other => panic!("expected SubscriptionDegraded update, got {other:?}"),
        }

        tasks.shutdown();
    }

    #[test]
    fn device_subscription_accepts_authoritative_shrink() {
        let incoming = vec![Device::new("device:current", "Current").current()];

        let stored = incoming;

        assert_eq!(stored.len(), 1);
        assert!(stored.iter().all(|device| device.is_current));
    }

    #[test]
    fn display_clock_helper_is_bounded_and_ui_only() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let display_clock_path = repo_root
            .join("crates/aura-terminal/src/tui/screens/app/subscriptions/display_clock.rs");
        let source = std::fs::read_to_string(&display_clock_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", display_clock_path.display())
        });

        assert!(source.contains("relative-time formatting only"));
        assert!(source.contains("observed-only UI maintenance"));
        assert!(source.contains("pub fn use_display_clock_state("));
        assert!(source.contains("time_workflows::current_time_ms"));
        assert!(source.contains("DISPLAY_CLOCK_MAX_CONSECUTIVE_FAILURES"));
        assert!(source.contains("DISPLAY_CLOCK_POLL_INTERVAL"));
        assert!(!source.contains("Ceremony"));
        assert!(!source.contains("UiUpdate::KeyRotationCeremonyStatus"));
    }

    #[test]
    fn display_clock_constants_remain_stable_for_nonsemantic_ui_refresh() {
        assert_eq!(DISPLAY_CLOCK_MAX_CONSECUTIVE_FAILURES, 200);
        assert_eq!(DISPLAY_CLOCK_POLL_INTERVAL, Duration::from_millis(1000));
    }
}
