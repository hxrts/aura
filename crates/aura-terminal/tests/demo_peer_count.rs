#![allow(clippy::expect_used, clippy::unwrap_used)]
//! Demo-mode peer count regression test.
//!
//! The TUI footer peer count is driven by `CONNECTION_STATUS_SIGNAL`, which is refreshed
//! by `aura_app::workflows::system::refresh_account()`.
//!
//! Peer count should represent **how many of your contacts are online**.
//! In demo mode, once Bob has Alice + Carol as contacts and their demo agents are running,
//! `refresh_account()` should emit `Online { peer_count: 2 }`.

use async_lock::RwLock;
use std::sync::Arc;
use std::time::Duration;

use aura_agent::{AgentBuilder, AgentConfig, EffectContext};
use aura_app::runtime_bridge::RuntimeBridge;
use aura_app::signal_defs::{ConnectionStatus, CONNECTION_STATUS_SIGNAL, CONTACTS_SIGNAL};
use aura_app::{AppConfig, AppCore};
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::effects::ExecutionMode;
use aura_core::hash;
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_journal::DomainFact;
use aura_relational::ContactFact;
use aura_terminal::demo::DemoSimulator;
use aura_terminal::tui::context::InitializedAppCore;
use aura_terminal::{handlers::tui::TuiMode, ids};
use uuid::Uuid;

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
            .all(|id| state.contacts.iter().any(|c| c.id == *id))
        {
            return;
        }

        if start.elapsed() > Duration::from_secs(2) {
            panic!(
                "Timed out waiting for contacts; expected={:?}, got={:?}",
                expected,
                state.contacts.iter().map(|c| c.id).collect::<Vec<_>>()
            );
        }

        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[tokio::test]
async fn demo_refresh_account_reports_two_online_contacts() {
    let seed = 2024u64;

    // Match the demo-mode authority/context derivation used by the TUI handler.
    let bob_device_id_str = "demo:bob";
    let bob_authority_entropy = hash::hash(format!("authority:{}", bob_device_id_str).as_bytes());
    let authority_id = AuthorityId::new_from_entropy(bob_authority_entropy);
    let bob_context_entropy = hash::hash(format!("context:{}", bob_device_id_str).as_bytes());
    let context_id = ContextId::new_from_entropy(bob_context_entropy);

    // Use a unique data dir so this test is hermetic.
    let test_dir = std::env::temp_dir().join(format!("aura-demo-peer-count-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&test_dir).expect("Failed to create test dir");

    // Start demo peers (Alice + Carol) as real runtimes and share their transport with Bob.
    let mut simulator = DemoSimulator::new(seed, test_dir.clone())
        .await
        .expect("Failed to create demo simulator");
    simulator.start().await.expect("Failed to start demo simulator");
    let shared_transport = simulator.shared_transport();

    let mut agent_config = AgentConfig::default();
    agent_config.device_id = ids::device_id(bob_device_id_str);
    agent_config.storage.base_path = test_dir.clone();

    let effect_ctx = EffectContext::new(authority_id, context_id, ExecutionMode::Simulation { seed });

    let agent = AgentBuilder::new()
        .with_config(agent_config)
        .with_authority(authority_id)
        .build_simulation_async_with_shared_transport(seed, &effect_ctx, shared_transport)
        .await
        .expect("Failed to build demo simulation agent");
    let agent = Arc::new(agent);

    let app_config = AppConfig {
        data_dir: test_dir.to_string_lossy().to_string(),
        ..AppConfig::default()
    };
    let app_core = AppCore::with_runtime(app_config, agent.clone().as_runtime_bridge())
        .expect("Failed to create AppCore with runtime");
    let app_core = Arc::new(RwLock::new(app_core));

    let initialized = InitializedAppCore::new(app_core.clone()).await.expect("init signals");

    // Make Alice + Carol contacts (facts), then wait for CONTACTS_SIGNAL to reflect them.
    let alice_id = simulator.alice_authority();
    let carol_id = simulator.carol_authority();

    let contact_facts = vec![
        ContactFact::added_with_timestamp_ms(
            ContextId::default(),
            authority_id,
            alice_id,
            "Alice".to_string(),
            1,
        )
        .to_generic(),
        ContactFact::added_with_timestamp_ms(
            ContextId::default(),
            authority_id,
            carol_id,
            "Carol".to_string(),
            2,
        )
        .to_generic(),
    ];

    agent
        .clone()
        .as_runtime_bridge()
        .commit_relational_facts(&contact_facts)
        .await
        .expect("commit contacts");

    wait_for_contacts(initialized.raw(), &[alice_id, carol_id]).await;

    // Run the same refresh path the TUI uses; it should emit Online{2}.
    aura_app::workflows::system::refresh_account(initialized.raw())
        .await
        .expect("refresh_account should succeed");

    let status = {
        let core = initialized.raw().read().await;
        core.read(&*CONNECTION_STATUS_SIGNAL)
            .await
            .expect("read CONNECTION_STATUS_SIGNAL")
    };

    assert_eq!(status, ConnectionStatus::Online { peer_count: 2 });

    // Keep TuiMode imported in this test file as a compile-time guard that
    // demo/prod mode remains a first-class concept in the public handler API.
    let _ = TuiMode::Demo { seed };

    simulator.stop().await.expect("Failed to stop demo simulator");
}
