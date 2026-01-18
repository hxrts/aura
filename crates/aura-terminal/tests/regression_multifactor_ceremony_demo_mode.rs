//! Regression test: Multifactor ceremony fails when adding mobile device in demo mode.
//!
//! This test replicates the bug where the TUI shows:
//! "Multifactor ceremony failed: Internal error: failed to st..."
//!
//! The issue occurs when:
//! 1. User runs TUI in demo mode
//! 2. User adds a mobile device (creates contact via DemoSimulator)
//! 3. User initiates multifactor authority setup with that mobile device
//! 4. The mobile device authority exists but doesn't have shared transport set up
//!
//! The ceremony fails because `send_envelope()` cannot deliver key packages to the
//! mobile device authority. The error message is truncated and unclear.
//!
//! This test should FAIL (panic) until the bug is fixed. The fix should either:
//! - Properly set up shared transport for mobile devices in demo mode
//! - Provide a clear error message explaining that the device is unreachable
//! - Gracefully handle the case when devices exist as contacts but aren't online

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

/// Regression test: Multifactor ceremony should fail gracefully when mobile device lacks transport.
///
/// This replicates the TUI bug where starting a multifactor ceremony with a mobile device
/// contact (created in demo mode but without shared transport) results in:
/// "Multifactor ceremony failed: Internal error: failed to st..."
///
/// Expected behavior: The ceremony should either:
/// 1. Properly set up transport for mobile devices (preferred for demo mode)
/// 2. Fail with a clear error about unreachable devices
/// 3. Start but timeout waiting for device responses
///
/// Current behavior: Fails immediately with truncated "failed to st..." message.
#[tokio::test]
async fn regression_multifactor_ceremony_fails_with_mobile_device_no_transport() {
    let seed = 3024u64;
    let test_dir = support::unique_test_dir("aura-multifactor-mobile-no-transport");

    // === Setup: Create Bob's authority/context matching demo pattern ===
    let bob_device_id_str = "demo:bob";
    let bob_authority_entropy = hash::hash(format!("authority:{}", bob_device_id_str).as_bytes());
    let bob_authority = AuthorityId::new_from_entropy(bob_authority_entropy);
    let bob_context_entropy = hash::hash(format!("context:{}", bob_device_id_str).as_bytes());
    let bob_context = ContextId::new_from_entropy(bob_context_entropy);

    // === Setup: Create mobile device authority (as demo would) ===
    // In demo mode, DemoSimulator creates a mobile device authority but the TUI user
    // adds it via contact fact without setting up shared transport for that device
    let mobile_device_id_str = "demo:bob-mobile";
    let mobile_authority_entropy =
        hash::hash(format!("authority:{}", mobile_device_id_str).as_bytes());
    let mobile_authority = AuthorityId::new_from_entropy(mobile_authority_entropy);

    let agent_config = AgentConfig {
        device_id: ids::device_id(bob_device_id_str),
        storage: aura_agent::core::config::StorageConfig {
            base_path: test_dir.clone(),
            ..Default::default()
        },
        ..Default::default()
    };

    let effect_ctx = EffectContext::new(bob_authority, bob_context, ExecutionMode::Simulation {
        seed,
    });

    // CRITICAL: Using build_simulation_async WITHOUT shared_transport
    // This means the mobile device authority won't have a running agent
    // This replicates the exact scenario from the TUI bug report
    let agent = AgentBuilder::new()
        .with_config(agent_config)
        .with_authority(bob_authority)
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

    // === Phase 1: Add mobile device as contact (mimicking TUI "add device" flow) ===
    // In the TUI, user would see the mobile device and add it as a contact
    // But the mobile device doesn't have shared transport set up
    let contact_facts = vec![ContactFact::added_with_timestamp_ms(
        ContextId::new_from_entropy([3u8; 32]),
        bob_authority,
        mobile_authority,
        "Bob's Mobile".to_string(),
        1,
    )
    .to_generic()];

    agent
        .clone()
        .as_runtime_bridge()
        .commit_relational_facts(&contact_facts)
        .await
        .expect("commit contact facts");

    // Give signal time to update
    tokio::time::sleep(Duration::from_millis(100)).await;

    // === Phase 2: Get current device ID and Bob's device ID ===
    // For multifactor, we need the DeviceId, not AuthorityId
    // The current device (Bob's laptop) should be participating
    let bob_device_id = ids::device_id(bob_device_id_str);
    let mobile_device_id = ids::device_id(mobile_device_id_str);

    // === Phase 3: Attempt multifactor ceremony (THIS IS WHERE THE BUG MANIFESTS) ===
    // The TUI calls initiate_device_threshold_ceremony with device IDs
    // Since the mobile device authority exists but has no running agent with shared transport,
    // the send_envelope() call at line 1519 in runtime_bridge/mod.rs fails

    let threshold = FrostThreshold::new(2).expect("valid threshold");
    let device_ids = vec![bob_device_id.to_string(), mobile_device_id.to_string()];

    let result = {
        let core = app_core.read().await;
        core.initiate_device_threshold_ceremony(threshold, 2, &device_ids)
            .await
    };

    // === Phase 4: Assert on the result ===
    // The ceremony should either succeed with proper transport setup
    // OR fail with a clear, helpful error message

    match result {
        Ok(ceremony_id) => {
            // If ceremony starts, check its status
            println!("Multifactor ceremony started with ID: {ceremony_id}");

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

                    if s.has_failed {
                        let error_msg = s.error_message.unwrap_or_default();

                        // Check if we got the improved error message
                        if error_msg.contains("device is not reachable")
                            || error_msg.contains("no running agent for device")
                        {
                            println!("SUCCESS: Clear error message about unreachable device");
                        } else {
                            panic!(
                                "REGRESSION: Ceremony failed with unclear error: {}",
                                error_msg
                            );
                        }
                    } else if !s.is_complete {
                        println!("Ceremony pending (acceptable - waiting for device responses)");
                    }
                }
                Err(e) => {
                    println!("Could not get ceremony status: {e}");
                }
            }
        }
        Err(e) => {
            let error_str = e.to_string();

            // Check for improved error message
            let has_improved_message = error_str.contains("device is not reachable")
                || error_str.contains("no running agent for device")
                || error_str.contains("mobile device not connected");

            if has_improved_message {
                println!(
                    "SUCCESS: Multifactor ceremony failed with clear error message:\n{error_str}"
                );
                // Test passes - the improved error message is present
            } else {
                // Check for the OLD cryptic error messages (regression)
                let is_failed_to_start = error_str.contains("Internal error")
                    && (error_str.contains("failed to st")
                        || error_str.contains("failed to send"));
                let is_send_envelope_error = error_str.contains("Failed to send device threshold")
                    || error_str.contains("send_envelope");
                let is_generic_internal_error = error_str.contains("Internal error")
                    && error_str.contains("Failed to register ceremony");

                if is_failed_to_start || is_send_envelope_error || is_generic_internal_error {
                    panic!(
                        "REGRESSION: Multifactor ceremony failed with cryptic error message.\n\n\
                         Error: {error_str}\n\n\
                         The error should clearly explain that the mobile device is not reachable.\n\
                         This matches the TUI bug: 'Multifactor ceremony failed: Internal error: failed to st...'"
                    );
                }

                // Other errors might be legitimate - still fail to capture them
                panic!("Multifactor ceremony failed with unexpected error: {error_str}");
            }
        }
    }

    // Cleanup
    let _ = std::fs::remove_dir_all(&test_dir);
}

/// Control test: Multifactor ceremony should work when mobile device has shared transport.
///
/// This test verifies that when the mobile device is properly set up with shared transport
/// (as it should be in demo mode), the multifactor ceremony completes successfully.
#[tokio::test]
async fn control_multifactor_ceremony_works_with_shared_transport() {
    use aura_terminal::demo::DemoSimulator;

    let seed = 3025u64;
    let test_dir = support::unique_test_dir("aura-multifactor-with-transport");

    let bob_device_id_str = "demo:bob";
    let bob_authority_entropy = hash::hash(format!("authority:{}", bob_device_id_str).as_bytes());
    let bob_authority = AuthorityId::new_from_entropy(bob_authority_entropy);
    let bob_context_entropy = hash::hash(format!("context:{}", bob_device_id_str).as_bytes());
    let bob_context = ContextId::new_from_entropy(bob_context_entropy);

    // Start demo simulator WITH shared transport
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

    let effect_ctx = EffectContext::new(bob_authority, bob_context, ExecutionMode::Simulation {
        seed,
    });

    // Build WITH shared transport
    let agent = AgentBuilder::new()
        .with_config(agent_config)
        .with_authority(bob_authority)
        .build_simulation_async_with_shared_transport(seed, &effect_ctx, shared_transport.clone())
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

    // Create a mobile device agent with shared transport
    // This simulates what DemoSimulator should do for mobile devices
    let mobile_device_id_str = "demo:bob-mobile";
    let mobile_authority_entropy =
        hash::hash(format!("authority:{}", mobile_device_id_str).as_bytes());
    let mobile_authority = AuthorityId::new_from_entropy(mobile_authority_entropy);
    let mobile_context_entropy =
        hash::hash(format!("context:{}", mobile_device_id_str).as_bytes());
    let mobile_context = ContextId::new_from_entropy(mobile_context_entropy);

    let mobile_agent_config = AgentConfig {
        device_id: ids::device_id(mobile_device_id_str),
        storage: aura_agent::core::config::StorageConfig {
            base_path: test_dir.join("mobile"),
            ..Default::default()
        },
        ..Default::default()
    };

    let mobile_effect_ctx = EffectContext::new(
        mobile_authority,
        mobile_context,
        ExecutionMode::Simulation { seed: seed + 1 },
    );

    let mobile_agent = AgentBuilder::new()
        .with_config(mobile_agent_config)
        .with_authority(mobile_authority)
        .build_simulation_async_with_shared_transport(
            seed + 1,
            &mobile_effect_ctx,
            shared_transport,
        )
        .await
        .expect("build mobile agent");
    let mobile_agent = Arc::new(mobile_agent);

    // Add mobile device as contact
    let contact_facts = vec![ContactFact::added_with_timestamp_ms(
        ContextId::new_from_entropy([3u8; 32]),
        bob_authority,
        mobile_authority,
        "Bob's Mobile".to_string(),
        1,
    )
    .to_generic()];

    agent
        .clone()
        .as_runtime_bridge()
        .commit_relational_facts(&contact_facts)
        .await
        .expect("commit contact facts");

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Start multifactor ceremony with both devices
    let bob_device_id = ids::device_id(bob_device_id_str);
    let mobile_device_id = ids::device_id(mobile_device_id_str);

    let ceremony_id = {
        let core = app_core.read().await;
        let threshold = FrostThreshold::new(2).expect("valid threshold");
        core.initiate_device_threshold_ceremony(
            threshold,
            2,
            &[bob_device_id.to_string(), mobile_device_id.to_string()],
        )
        .await
        .expect("initiate_device_threshold_ceremony should succeed with shared transport")
    };

    println!("Control test: Multifactor ceremony started with ID: {ceremony_id}");

    // Start ceremony processors for both devices
    let bob_ceremony_agent = agent.clone();
    let bob_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        loop {
            interval.tick().await;
            let _ = bob_ceremony_agent.process_ceremony_acceptances().await;
        }
    });

    let mobile_ceremony_agent = mobile_agent.clone();
    let mobile_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        loop {
            interval.tick().await;
            let _ = mobile_ceremony_agent.process_ceremony_acceptances().await;
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
            let error_signal = support::read_error_signal(&app_core).await;
            panic!(
                "Control test failed: {:?}, error_signal={error_signal:?}",
                status.error_message
            );
        }

        if status.is_complete {
            println!("Control test: Multifactor ceremony completed successfully");
            break;
        }

        if start.elapsed() > Duration::from_secs(20) {
            panic!("Control test: Timed out waiting for multifactor ceremony completion")
        }
    }

    bob_task.abort();
    mobile_task.abort();
    simulator.stop().await.expect("stop demo simulator");
    let _ = std::fs::remove_dir_all(&test_dir);
}
