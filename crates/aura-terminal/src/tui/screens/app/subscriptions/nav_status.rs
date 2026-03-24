use super::*;

use crate::tui::screens::app::subscriptions::contracts::{
    subscribe_lifecycle_signal, subscribe_update_bridge_signal, StructuralDegradationSink,
};

/// Shared authority id state for UI dispatch handlers.
#[derive(Clone, Default)]
pub struct SharedAuthorityId(Arc<RwLock<Option<AuthorityId>>>);

impl SharedAuthorityId {
    #[must_use]
    pub fn new() -> Self {
        Self(Arc::new(RwLock::new(None)))
    }

    pub fn read(&self) -> parking_lot::RwLockReadGuard<'_, Option<AuthorityId>> {
        self.0.read()
    }

    pub fn write(&self) -> parking_lot::RwLockWriteGuard<'_, Option<AuthorityId>> {
        self.0.write()
    }
}

pub fn use_authority_id_subscription(
    hooks: &mut Hooks,
    app_ctx: &AppCoreContext,
    update_tx: Option<UiUpdateSender>,
) -> SharedAuthorityId {
    let shared_ref = hooks.use_ref(SharedAuthorityId::new);
    let shared: SharedAuthorityId = shared_ref.read().clone();
    let tasks = app_ctx.tasks();

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let authority_id = shared.clone();
        let degradation = StructuralDegradationSink::new(tasks.clone(), update_tx.clone());
        async move {
            subscribe_update_bridge_signal(
                app_core,
                &*SETTINGS_SIGNAL,
                move |settings_state| {
                    *authority_id.write() = settings_state.authority_id.parse::<AuthorityId>().ok();
                    if let Some(ref tx) = update_tx {
                        let current_index = settings_state
                            .authorities
                            .iter()
                            .position(|authority| authority.is_current)
                            .unwrap_or(0);
                        let authorities = settings_state
                            .authorities
                            .iter()
                            .map(|authority| {
                                let info = AuthorityInfo::new(
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
                        let _ = tx.try_send(UiUpdate::AuthoritiesUpdated {
                            authorities,
                            current_index,
                        });
                    }
                },
                degradation,
            )
            .await;
        }
    });

    shared
}

pub struct NavStatusSignals {
    pub network_status: State<NetworkStatus>,
    pub known_online: State<usize>,
    pub transport_peers: State<usize>,
    pub now_ms: State<Option<u64>>,
}

pub fn use_nav_status_signals(
    hooks: &mut Hooks,
    app_ctx: &AppCoreContext,
    initial_network_status: NetworkStatus,
    initial_known_online: usize,
    initial_transport_peers: usize,
) -> NavStatusSignals {
    let network_status = hooks.use_state(|| initial_network_status);
    let known_online = hooks.use_state(|| initial_known_online);
    let transport_peers = hooks.use_state(|| initial_transport_peers);
    let now_ms = use_display_clock_state(hooks, app_ctx);
    let tasks = app_ctx.tasks();

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let mut network_status = network_status.clone();
        let degradation = StructuralDegradationSink::new(tasks.clone(), None);
        async move {
            subscribe_update_bridge_signal(
                app_core,
                &*NETWORK_STATUS_SIGNAL,
                move |status| {
                    if network_status.get() != status {
                        network_status.set(status);
                    }
                },
                degradation,
            )
            .await;
        }
    });

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let mut known_online = known_online.clone();
        let degradation = StructuralDegradationSink::new(tasks.clone(), None);
        async move {
            subscribe_update_bridge_signal(
                app_core,
                &*CONNECTION_STATUS_SIGNAL,
                move |connection_status| {
                    let online = match connection_status {
                        ConnectionStatus::Offline => 0,
                        ConnectionStatus::Connecting => 0,
                        ConnectionStatus::Online { peer_count } => peer_count,
                    };
                    if known_online.get() != online {
                        known_online.set(online);
                    }
                },
                degradation,
            )
            .await;
        }
    });

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let mut transport_peers = transport_peers.clone();
        let degradation = StructuralDegradationSink::new(tasks, None);
        async move {
            subscribe_lifecycle_signal(
                app_core,
                &*TRANSPORT_PEERS_SIGNAL,
                move |count| {
                    if transport_peers.get() != count {
                        transport_peers.set(count);
                    }
                },
                degradation,
            )
            .await;
        }
    });

    NavStatusSignals {
        network_status,
        known_online,
        transport_peers,
        now_ms,
    }
}
