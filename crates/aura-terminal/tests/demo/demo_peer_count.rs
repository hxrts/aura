#![cfg(feature = "development")]
#![allow(clippy::expect_used, clippy::unwrap_used)]
//! Demo-mode peer count regression test.
//!
//! The TUI footer peer count is driven by `CONNECTION_STATUS_SIGNAL`, which is refreshed
//! by `aura_app::ui::workflows::system::refresh_account()`.
//!
//! Peer count should represent **how many of your contacts are online**.
//! In demo mode, once Bob has Alice + Carol as contacts and their demo agents are running,
//! `refresh_account()` should emit `Online { peer_count: 2 }`.

use async_lock::RwLock;
use std::sync::Arc;
use std::time::Duration;

use aura_app::signal_defs::{ConnectionStatus, CONNECTION_STATUS_SIGNAL, CONTACTS_SIGNAL};
use aura_app::AppCore;
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::types::identifiers::AuthorityId;

#[allow(clippy::duplicate_mod)]
#[path = "../support/mod.rs"]
mod support;

use support::{FullTestEnv, FullTestEnvConfig};

async fn demo_env(name: &str) -> FullTestEnv {
    FullTestEnv::with_config(FullTestEnvConfig {
        name: name.to_string(),
        nickname_suggestion: Some("Bob".to_string()),
        with_demo_peers: true,
        ..Default::default()
    })
    .await
}

async fn wait_for_contacts(app_core: &Arc<RwLock<AppCore>>, expected: &[AuthorityId]) {
    let start = tokio::time::Instant::now();
    loop {
        let state = {
            let core = app_core.read().await;
            core.read(&*CONTACTS_SIGNAL)
                .await
                .expect("read CONTACTS_SIGNAL")
        };
        if expected
            .iter()
            .all(|id| state.all_contacts().any(|c| c.id == *id))
        {
            return;
        }
        if start.elapsed() > Duration::from_secs(10) {
            panic!(
                "Timed out waiting for contacts; expected={expected:?}, got={:?}",
                state.all_contacts().map(|c| c.id).collect::<Vec<_>>()
            );
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

/// Import and accept both demo peers' contact codes through the TUI's path.
async fn add_demo_peers_as_contacts(env: &FullTestEnv) -> [AuthorityId; 2] {
    let peers = env.demo_peers.as_ref().expect("demo peers started");
    let (alice_code, carol_code) = peers
        .signed_contact_invite_codes()
        .await
        .expect("demo peers create signed contact codes");
    for code in [&alice_code, &carol_code] {
        let invitation =
            aura_app::ui::workflows::invitation::import_invitation_details(&env.app_core, code)
                .await
                .expect("import_invitation_details should succeed");
        aura_app::ui::workflows::invitation::accept_invitation(&env.app_core, invitation)
            .await
            .expect("accept_invitation should succeed");
    }
    let ids = [peers.alice_authority(), peers.carol_authority()];
    wait_for_contacts(&env.app_core, &ids).await;
    ids
}

async fn connection_status(env: &FullTestEnv) -> ConnectionStatus {
    // The TUI refreshes connection status on its account refresh tick.
    aura_app::ui::workflows::system::refresh_account(&env.app_core)
        .await
        .expect("refresh_account should succeed");
    let core = env.app_core.read().await;
    core.read(&*CONNECTION_STATUS_SIGNAL)
        .await
        .expect("read CONNECTION_STATUS_SIGNAL")
}

#[tokio::test]
async fn demo_refresh_account_reports_two_online_contacts() {
    let env = demo_env("peer-count-refresh").await;
    add_demo_peers_as_contacts(&env).await;
    assert_eq!(
        connection_status(&env).await,
        ConnectionStatus::Online { peer_count: 2 }
    );
}

#[tokio::test]
async fn demo_accepting_contact_invites_updates_peer_count() {
    let env = demo_env("peer-count-invites").await;
    let before = connection_status(&env).await;
    assert!(
        !matches!(before, ConnectionStatus::Online { peer_count: 2 }),
        "no contacts yet: {before:?}"
    );
    add_demo_peers_as_contacts(&env).await;
    assert_eq!(
        connection_status(&env).await,
        ConnectionStatus::Online { peer_count: 2 }
    );
}
