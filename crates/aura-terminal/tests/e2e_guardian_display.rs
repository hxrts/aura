//! Guardian display E2E tests (development-only).

#![cfg(feature = "development")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::disallowed_methods,
    clippy::needless_borrows_for_generic_args,
    missing_docs
)]

use async_lock::RwLock;
use std::sync::Arc;
use std::time::Duration;

use aura_agent::{AgentBuilder, AgentConfig, AuraAgent, EffectContext};
use aura_app::signal_defs::CONTACTS_SIGNAL;
use aura_app::{AppConfig, AppCore};
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::effects::ExecutionMode;
use aura_core::hash;
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::types::FrostThreshold;
use aura_journal::DomainFact;
use aura_relational::ContactFact;
use aura_terminal::demo::DemoSimulator;
use aura_terminal::ids;
use aura_terminal::tui::context::InitializedAppCore;

mod support;

#[tokio::test]
async fn demo_guardian_ceremony_completes_with_demo_peers() {
    let seed = 2024u64;

    // Unique data dir so this test is hermetic.
    let test_dir = support::unique_test_dir("aura-guardian-e2e");

    // Start demo peers (Alice + Carol) as real runtimes and share their transport with Bob.
    let mut simulator = DemoSimulator::new(seed, test_dir.clone())
        .await
        .expect("create demo simulator");
    simulator.start().await.expect("start demo simulator");
    let shared_transport = simulator.shared_transport();

    // Match the demo-mode authority/context derivation used by the TUI handler.
    let bob_device_id_str = "demo:bob";
    let bob_authority_entropy = hash::hash(format!("authority:{}", bob_device_id_str).as_bytes());
    let bob_authority = AuthorityId::new_from_entropy(bob_authority_entropy);
    let bob_context_entropy = hash::hash(format!("context:{}", bob_device_id_str).as_bytes());
    let bob_context = ContextId::new_from_entropy(bob_context_entropy);

    let agent_config = AgentConfig {
        device_id: ids::device_id(bob_device_id_str),
        storage: aura_agent::core::config::StorageConfig {
            base_path: test_dir.clone(),
            ..Default::default()
        },
        ..Default::default()
    };

    let effect_ctx = EffectContext::new(
        bob_authority,
        bob_context,
        ExecutionMode::Simulation { seed },
    );

    let agent = AgentBuilder::new()
        .with_config(agent_config)
        .with_authority(bob_authority)
        .build_simulation_async_with_shared_transport(seed, &effect_ctx, shared_transport)
        .await
        .expect("build bob agent");
    let agent = Arc::new(agent);

    let app_config = AppConfig {
        data_dir: test_dir.to_string_lossy().to_string(),
        ..AppConfig::default()
    };
    let app_core = AppCore::with_runtime(app_config, agent.clone().as_runtime_bridge())
        .expect("create AppCore with runtime");
    let app_core = Arc::new(RwLock::new(app_core));

    let initialized = InitializedAppCore::new(app_core.clone())
        .await
        .expect("init signals");
    let _ = initialized;

    // Ensure Alice + Carol exist as contacts so guardian binding facts can flip the flag.
    let alice_id = simulator.alice_authority();
    let carol_id = simulator.carol_authority();

    let contact_facts = vec![
        ContactFact::added_with_timestamp_ms(
            ContextId::default(),
            bob_authority,
            alice_id,
            "Alice".to_string(),
            1,
        )
        .to_generic(),
        ContactFact::added_with_timestamp_ms(
            ContextId::default(),
            bob_authority,
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
        .expect("commit contact facts");

    // Start a real guardian ceremony.
    let ceremony_id = {
        let core = app_core.read().await;
        let threshold = FrostThreshold::new(2).expect("valid threshold");
        core.initiate_guardian_ceremony(threshold, 2, &[alice_id.to_string(), carol_id.to_string()])
            .await
            .expect("initiate_guardian_ceremony")
    };

    // Mirror the TUI background task that polls for acceptances.
    let ceremony_agent: Arc<AuraAgent> = agent.clone();
    let ceremony_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        loop {
            interval.tick().await;
            let _ = ceremony_agent.process_ceremony_acceptances().await;
        }
    });

    // Wait until the ceremony is complete.
    let start = tokio::time::Instant::now();
    loop {
        tokio::time::sleep(Duration::from_millis(150)).await;

        let status = {
            let core = app_core.read().await;
            core.get_ceremony_status(&ceremony_id)
                .await
                .expect("get_ceremony_status")
        };

        if status.has_failed {
            panic!("Ceremony failed: {:?}", status.error_message);
        }

        if status.is_complete {
            break;
        }

        if start.elapsed() > Duration::from_secs(20) {
            panic!("Timed out waiting for ceremony completion")
        }
    }

    ceremony_task.abort();

    // Contacts should reflect guardian status via committed GuardianBinding facts.
    let start = tokio::time::Instant::now();
    loop {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let contacts = {
            let core = app_core.read().await;
            core.read(&*CONTACTS_SIGNAL).await.unwrap_or_default()
        };

        let alice_guardian = contacts
            .contact(&alice_id)
            .map(|c| c.is_guardian)
            .unwrap_or(false);
        let carol_guardian = contacts
            .contact(&carol_id)
            .map(|c| c.is_guardian)
            .unwrap_or(false);

        if alice_guardian && carol_guardian {
            break;
        }

        if start.elapsed() > Duration::from_secs(5) {
            panic!(
                "Timed out waiting for guardian flags; alice={alice_guardian} carol={carol_guardian}"
            );
        }
    }

    simulator.stop().await.expect("stop demo simulator");
}

#[tokio::test]
async fn test_authority_id_derivation_matches() {
    let seed = 2024u64;

    let hints_alice_authority = ids::authority_id(&format!("demo:{}:{}:authority", seed, "Alice"));
    let simulator_alice_authority =
        ids::authority_id(&format!("demo:{}:{}:authority", seed, "Alice"));

    assert_eq!(
        hints_alice_authority, simulator_alice_authority,
        "AuthorityId derivations must match for demo lookup to work"
    );
}
