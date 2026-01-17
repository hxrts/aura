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
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::types::FrostThreshold;
use aura_journal::DomainFact;
use aura_relational::ContactFact;
use aura_terminal::ids;
use aura_terminal::tui::context::InitializedAppCore;

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
    let authority_entropy = hash::hash(format!("authority:{}", device_id_str).as_bytes());
    let authority_id = AuthorityId::new_from_entropy(authority_entropy);
    let context_entropy = hash::hash(format!("context:{}", device_id_str).as_bytes());
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

    // === Phase 2: Attempt guardian ceremony (THIS IS WHERE THE BUG MANIFESTS) ===
    // The TUI calls initiate_guardian_ceremony with the contact IDs
    // Since no DemoSimulator is running with shared transport,
    // there are no actual peer agents to communicate with

    let threshold = FrostThreshold::new(2).expect("valid threshold");
    let guardian_ids = vec![alice_id.to_string(), carol_id.to_string()];

    let result = {
        let core = app_core.read().await;
        core.initiate_guardian_ceremony(threshold, 2, &guardian_ids)
            .await
    };

    // === Phase 3: Assert on the result ===
    // The ceremony should fail with a clear error message about peers being unreachable.

    match result {
        Ok(ceremony_id) => {
            // If ceremony starts, we should be able to check its status
            // It might start but then fail/timeout waiting for responses
            println!("Ceremony started with ID: {ceremony_id}");

            // Wait a bit and check status
            tokio::time::sleep(Duration::from_millis(500)).await;

            let status = {
                let core = app_core.read().await;
                core.get_ceremony_status(&ceremony_id).await
            };

            match status {
                Ok(s) => {
                    println!(
                        "Ceremony status: complete={}, failed={}, error={:?}",
                        s.is_complete, s.has_failed, s.error_message
                    );

                    // Ceremony should either be pending (waiting for responses that will never come)
                    // or failed due to timeout/unreachable peers
                    // Either is acceptable behavior - the point is it shouldn't fail to start
                    if s.has_failed {
                        println!("Ceremony failed (may be expected): {:?}", s.error_message);
                    } else if !s.is_complete {
                        println!("Ceremony is pending (waiting for unreachable peers)");
                    }
                }
                Err(e) => {
                    println!("Could not get ceremony status: {e}");
                }
            }
        }
        Err(e) => {
            let error_str = e.to_string();

            // Check for the IMPROVED error message that explains the issue clearly
            let has_improved_message = error_str.contains("no responses received from guardians")
                && error_str.contains("Ensure guardian peers are online and connected");

            if has_improved_message {
                // SUCCESS: The error message now clearly explains the problem
                println!(
                    "SUCCESS: Guardian ceremony failed with clear error message:\n{error_str}"
                );
                // Test passes - the improved error message is present
            } else {
                // Check for the OLD cryptic error messages (regression)
                let is_failed_to_start =
                    error_str.contains("internal error") && error_str.contains("failed to start");
                let is_no_message_provider = error_str.contains("message provider returned None");
                let is_protocol_violation = error_str.contains("Protocol violation");

                if is_failed_to_start || is_no_message_provider || is_protocol_violation {
                    panic!(
                        "REGRESSION: Guardian ceremony failed with cryptic error message.\n\n\
                         Error: {error_str}\n\n\
                         The error should explain that guardian peers are unreachable."
                    );
                }

                // Other errors might be legitimate - still fail to capture them
                panic!("Guardian ceremony failed with unexpected error: {error_str}");
            }
        }
    }

    // Cleanup
    let _ = std::fs::remove_dir_all(&test_dir);
}

/// Control test: Guardian ceremony should work when DemoSimulator peers are running.
///
/// This test currently FAILS because DemoSimulator does not automatically respond
/// to guardian ceremony requests. This is the next issue to fix.
#[tokio::test]
async fn control_guardian_ceremony_works_with_demo_peers() {
    use aura_core::hash;
    use aura_core::identifiers::ContextId;
    use aura_journal::DomainFact;
    use aura_relational::ContactFact;
    use aura_terminal::demo::DemoSimulator;

    let seed = 2024u64;
    let test_dir = support::unique_test_dir("aura-guardian-with-peers-control");

    // Match the demo-mode authority/context derivation
    let bob_device_id_str = "demo:bob";
    let bob_authority_entropy = hash::hash(format!("authority:{}", bob_device_id_str).as_bytes());
    let bob_authority =
        aura_core::identifiers::AuthorityId::new_from_entropy(bob_authority_entropy);
    let bob_context_entropy = hash::hash(format!("context:{}", bob_device_id_str).as_bytes());
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

    // Start guardian ceremony - this should work
    let ceremony_id = {
        let core = app_core.read().await;
        let threshold = FrostThreshold::new(2).expect("valid threshold");
        core.initiate_guardian_ceremony(threshold, 2, &[alice_id.to_string(), carol_id.to_string()])
            .await
            .expect("initiate_guardian_ceremony should succeed with demo peers")
    };

    println!("Control test: Ceremony started with ID: {ceremony_id}");

    // Start ceremony processor
    let ceremony_agent = agent.clone();
    let ceremony_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        loop {
            interval.tick().await;
            let _ = ceremony_agent.process_ceremony_acceptances().await;
        }
    });

    // Wait for completion
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

    ceremony_task.abort();
    simulator.stop().await.expect("stop demo simulator");
    let _ = std::fs::remove_dir_all(&test_dir);
}
