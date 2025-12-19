//! iocraft hook helpers for long-lived reactive subscriptions.
//!
//! Keep `shell.rs` focused on wiring and rendering by extracting the
//! signal-subscription `use_future` blocks here.

use iocraft::prelude::*;

use aura_app::signal_defs::{
    ConnectionStatus, SyncStatus, CONNECTION_STATUS_SIGNAL, SYNC_STATUS_SIGNAL,
};
use aura_core::effects::reactive::ReactiveEffects;

use crate::tui::hooks::AppCoreContext;

pub struct NavStatusSignals {
    pub syncing: State<bool>,
    pub peer_count: State<usize>,
    pub last_sync_time: State<Option<u64>>,
}

pub fn use_nav_status_signals(
    hooks: &mut Hooks,
    app_ctx: &AppCoreContext,
    initial_syncing: bool,
    initial_peer_count: usize,
    initial_last_sync_time: Option<u64>,
) -> NavStatusSignals {
    let syncing = hooks.use_state(|| initial_syncing);
    let peer_count = hooks.use_state(|| initial_peer_count);
    let last_sync_time = hooks.use_state(|| initial_last_sync_time);

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let mut syncing = syncing.clone();
        async move {
            let mut stream = {
                let core = app_core.read().await;
                core.subscribe(&*SYNC_STATUS_SIGNAL)
            };
            while let Ok(status) = stream.recv().await {
                syncing.set(matches!(status, SyncStatus::Syncing { .. }));
            }
        }
    });

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let mut peer_count = peer_count.clone();
        async move {
            let mut stream = {
                let core = app_core.read().await;
                core.subscribe(&*CONNECTION_STATUS_SIGNAL)
            };
            while let Ok(status) = stream.recv().await {
                let peers = match status {
                    ConnectionStatus::Online { peer_count } => peer_count,
                    _ => 0,
                };
                peer_count.set(peers);
            }
        }
    });

    NavStatusSignals {
        syncing,
        peer_count,
        last_sync_time,
    }
}
