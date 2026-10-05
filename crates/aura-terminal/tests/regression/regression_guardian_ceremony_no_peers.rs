//! Regression test: Guardian ceremony fails when contacts exist but no demo peers are running.
//!
//! This test replicates the bug where the TUI shows:
//! "guardian ceremony failed: internal error: failed to start"
//!
//! The issue occurs when:
//! 1. User has contacts (created via ContactFact)
//! 2. User starts guardian setup ceremony with those contacts
//! 3. No DemoSimulator or shared transport is running (no actual peers to respond)
//!
//! The ceremony fails because there are no actual peer agents to communicate with.
//!
//! This test should FAIL (panic) until the bug is fixed. The fix should either:
//! - Provide a better error message explaining that peers are unreachable
//! - Or handle the case gracefully when contacts don't have running agents

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

use aura_agent::core::{AgentBuilder, AgentConfig};
use aura_agent::EffectContext;
use aura_app::{AppConfig, AppCore};
use aura_core::effects::ExecutionMode;
use aura_core::hash;
use aura_core::types::identifiers::{AuthorityId, ContextId};
use aura_core::types::FrostThreshold;
use aura_journal::DomainFact;
use aura_relational::ContactFact;
use aura_terminal::ids;
use aura_terminal::tui::context::InitializedAppCore;

#[allow(clippy::duplicate_mod)]
#[path = "../support/mod.rs"]
mod support;
use support::read_error_signal;

/// Regression test: Guardian ceremony should fail gracefully when peers don't exist.
///
/// This replicates the TUI bug where starting a guardian ceremony with contacts
/// (but no running demo peers with shared transport) results in
/// "guardian ceremony failed: internal error: failed to start".
///
/// Expected behavior: The ceremony should either:
/// 1. Start and timeout waiting for responses (acceptable)
/// 2. Fail immediately with a clear error about unreachable peers (preferred)
///
/// Current behavior: Fails with cryptic "internal error: failed to start" message.
#[tokio::test]
async fn regression_guardian_ceremony_fails_without_demo_peers() {
    let seed = 2024u64;
    let test_dir = support::unique_test_dir("aura-guardian-no-peers-regression");

    // === Setup: Create authority/context matching demo pattern ===
    let device_id_str = "demo:bob";
    let authority_entropy = hash::hash(format!("authority:{device_id_str}").as_bytes());
    let authority_id = AuthorityId::new_from_entropy(authority_entropy);
    let context_entropy = hash::hash(format!("context:{device_id_str}").as_bytes());
    let context_id = ContextId::new_from_entropy(context_entropy);

    let agent_config = AgentConfig {
        device_id: ids::device_id(device_id_str),
        storage: aura_agent::core::config::StorageConfig {
            base_path: test_dir.clone(),
            ..Default::default()
        },
        ..Default::default()
    };

    let effect_ctx =
        EffectContext::new(authority_id, context_id, ExecutionMode::Simulation { seed });

    // CRITICAL: Using build_simulation_async WITHOUT shared_transport
    // This means Alice and Carol won't actually exist as running agents
    // This is different from e2e_guardian_display.rs which uses shared transport
    let agent = AgentBuilder::new()
        .with_config(agent_config)
        .with_authority(authority_id)
        .build_simulation_async(seed, &effect_ctx)
        .await
        .expect("Failed to build simulation agent");
    let agent = Arc::new(agent);

    let app_config = AppConfig {
        data_dir: test_dir.to_string_lossy().to_string(),
        ..AppConfig::default()
    };
    let app_core = AppCore::with_runtime(app_config, agent.clone().as_runtime_bridge())
        .expect("Failed to create AppCore with runtime");
    let app_core = Arc::new(RwLock::new(app_core));

    let _initialized = InitializedAppCore::new(app_core.clone())
        .await
        .expect("init signals");

    // === Phase 1: Create contacts via facts (same pattern as working test) ===
    // These are the same authority IDs that DemoSimulator would create
    let alice_id = ids::authority_id(&format!("demo:{seed}:Alice:authority"));
    let carol_id = ids::authority_id(&format!("demo:{}:Carol:authority", seed + 1));

    let contact_facts = vec![
        ContactFact::added_with_timestamp_ms(
            ContextId::new_from_entropy([2u8; 32]),
            authority_id,
            alice_id,
            "Alice".to_string(),
            1,
        )
        .to_generic(),
        ContactFact::added_with_timestamp_ms(
            ContextId::new_from_entropy([2u8; 32]),
            authority_id,
            carol_id,
            "Carol".to_string(),
            2,
        )
        .to_generic(),
    ];

    // Commit contacts to journal
    agent
        .clone()
        .as_runtime_bridge()
        .commit_relational_facts(&contact_facts)
        .await
        .expect("commit contact facts");

    // Give signal time to update
    tokio::time::sleep(Duration::from_millis(100)).await;

    // === Phase 2: Attempt guardian ceremony ===
    // Alice and Carol are contacts but never accepted a guardian invitation,
    // so there is no verified guardian key to run the ceremony against.
    let threshold = FrostThreshold::new(2).expect("valid threshold");
    let result = aura_app::ui::workflows::ceremonies::start_guardian_ceremony(
        &app_core,
        threshold,
        2,
        vec![alice_id, carol_id],
    )
    .await;

    // === Phase 3: Assert on the result ===
    // The ceremony is refused up front with a clear, actionable error instead
    // of starting and then failing silently (work/8.md Task 56) or failing
    // with a cryptic internal error (the original regression).
    let error = result.expect_err("guardians without verified keys must be refused");
    let error_str = error.to_string();
    assert!(
        error_str.contains("has not accepted a guardian invitation yet"),
        "expected the missing-guardian-key refusal, got: {error_str}"
    );
    for cryptic in [
        "failed to start",
        "message provider returned None",
        "Protocol violation",
    ] {
        assert!(!error_str.contains(cryptic), "cryptic error: {error_str}");
    }

    // Cleanup
    let _ = std::fs::remove_dir_all(&test_dir);
}

/// Control test: Guardian ceremony should work when DemoSimulator peers are running.
///
/// This test currently FAILS because DemoSimulator does not automatically respond
/// to guardian ceremony requests. This is the next issue to fix.
#[test]
fn control_guardian_ceremony_works_with_demo_peers() {
    support::run_with_terminal_stack(control_guardian_ceremony_works_with_demo_peers_body);
}

async fn control_guardian_ceremony_works_with_demo_peers_body() {
    use aura_core::hash;
    use aura_core::types::identifiers::ContextId;
    use aura_journal::DomainFact;
    use aura_relational::ContactFact;
    use aura_terminal::demo::DemoSimulator;

    let seed = 2024u64;
    let test_dir = support::unique_test_dir("aura-guardian-with-peers-control");

    // Match the demo-mode authority/context derivation
    let bob_device_id_str = "demo:bob";
    let bob_authority_entropy = hash::hash(format!("authority:{bob_device_id_str}").as_bytes());
    let bob_authority =
        aura_core::types::identifiers::AuthorityId::new_from_entropy(bob_authority_entropy);
    let bob_context_entropy = hash::hash(format!("context:{bob_device_id_str}").as_bytes());
    let bob_context = ContextId::new_from_entropy(bob_context_entropy);

    // Start demo peers WITH shared transport
    let mut simulator = DemoSimulator::new(seed, test_dir.clone(), bob_authority, bob_context)
        .await
        .expect("create demo simulator");
    simulator.start().await.expect("start demo simulator");
    let shared_transport = simulator.shared_transport();

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

    // Build WITH shared transport
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

    let _initialized = InitializedAppCore::new(app_core.clone())
        .await
        .expect("init signals");

    // Create contacts via direct fact commit (same as working test)
    let alice_id = simulator.alice_authority();
    let carol_id = simulator.carol_authority();

    let contact_facts = vec![
        ContactFact::added_with_timestamp_ms(
            ContextId::new_from_entropy([2u8; 32]),
            bob_authority,
            alice_id,
            "Alice".to_string(),
            1,
        )
        .to_generic(),
        ContactFact::added_with_timestamp_ms(
            ContextId::new_from_entropy([2u8; 32]),
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

    // Guardians must accept a guardian invitation first; that records the
    // verified key the ceremony needs (work/8.md Task 56). The demo peers
    // accept guardian bindings on their own.
    use aura_app::ui::workflows::{ceremonies, invitation};
    for guardian in [alice_id, carol_id] {
        invitation::create_guardian_invitation(&app_core, guardian, bob_authority, None, None)
            .await
            .expect("send guardian invitation to a demo peer");
    }

    // Start guardian ceremony once both guardians' keys are verified.
    let threshold = FrostThreshold::new(2).expect("valid threshold");
    let start = tokio::time::Instant::now();
    let ceremony = loop {
        match ceremonies::start_guardian_ceremony(&app_core, threshold, 2, vec![alice_id, carol_id])
            .await
        {
            Ok(handle) => break handle,
            Err(error)
                if error
                    .to_string()
                    .contains("has not accepted a guardian invitation yet")
                    && start.elapsed() < Duration::from_secs(30) =>
            {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            Err(error) => panic!("guardian ceremony should start with demo peers: {error}"),
        }
    };
    let status_handle = ceremony.status_handle();

    println!(
        "Control test: Ceremony started with ID: {}",
        status_handle.ceremony_id()
    );
    // Wait for completion
    let start = tokio::time::Instant::now();
    loop {
        tokio::time::sleep(Duration::from_millis(150)).await;

        let status = ceremonies::get_key_rotation_ceremony_status(&app_core, &status_handle)
            .await
            .expect("get ceremony status");

        if status.has_failed {
            let error_signal = read_error_signal(&app_core).await;
            panic!(
                "Control test failed: {:?}, error_signal={error_signal:?}",
                status.error_message
            );
        }

        if status.is_complete {
            println!("Control test: Ceremony completed successfully");
            break;
        }

        if start.elapsed() > Duration::from_secs(20) {
            panic!("Control test: Timed out waiting for ceremony completion")
        }
    }
    simulator.stop().await.expect("stop demo simulator");
    let _ = std::fs::remove_dir_all(&test_dir);
}
