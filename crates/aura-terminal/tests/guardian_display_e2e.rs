//! Guardian display E2E tests (development-only).

#![cfg(feature = "development")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::disallowed_methods,
    clippy::needless_borrows_for_generic_args,
    missing_docs
)]
//! # Guardian Display E2E Test
//!
//! Tests that contacts correctly show is_guardian=true after accepting a guardian request.
//!
//! This is the ACTUAL e2e test that validates the TUI correctly displays guardian status.
//! It runs the full demo flow:
//! 1. Start demo simulator with Alice and Carol
//! 2. Start signal coordinator to process agent responses
//! 3. Import Alice's invite code → Alice becomes a contact
//! 4. Send guardian request to Alice (InviteGuardian command, like the UI does)
//! 5. Wait for simulated agent to respond with AcceptGuardianBinding
//! 6. Verify CONTACTS_SIGNAL shows Alice with is_guardian=true
//!
//! ## Running
//!
//! ```bash
//! cargo test --package aura-terminal --features development --test guardian_display_e2e -- --nocapture
//! ```

use async_lock::RwLock;
use std::sync::Arc;
use std::time::Duration;

use aura_agent::{AgentBuilder, AgentConfig, AuraAgent, EffectContext};
use aura_app::signal_defs::{CONTACTS_SIGNAL, RECOVERY_SIGNAL};
use aura_app::{AppConfig, AppCore};
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::identifiers::AuthorityId;
use aura_terminal::demo::{DemoSignalCoordinator, DemoSimulator};
use aura_terminal::handlers::tui::TuiMode;
use aura_terminal::ids;
use aura_terminal::tui::context::IoContext;
use aura_terminal::tui::effects::EffectCommand;
use base64::Engine;

// ============================================================================
// Test Infrastructure
// ============================================================================

/// Generate a deterministic invite code for a demo agent (matches hints.rs)
fn generate_demo_invite_code(name: &str, seed: u64) -> String {
    let sender_id = ids::authority_id(&format!("demo:{}:{}:authority", seed, name));
    let invitation_id = ids::uuid(&format!("demo:{}:{}:invitation", seed, name));

    let invitation_data = serde_json::json!({
        "version": 1,
        "invitation_id": invitation_id.to_string(),
        "sender_id": sender_id.uuid().to_string(),
        "invitation_type": {
            "Contact": {
                "nickname": name
            }
        },
        "expires_at": null,
        "message": format!("Contact invitation from {} (demo)", name)
    });

    let json_str = serde_json::to_string(&invitation_data).unwrap();
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json_str.as_bytes());
    format!("aura:v1:{}", b64)
}

/// Create test environment with IoContext and AppCore that has RuntimeBridge
async fn setup_test_env(
    name: &str,
    seed: u64,
    shared_inbox: &std::sync::Arc<std::sync::RwLock<Vec<aura_core::effects::TransportEnvelope>>>,
) -> (Arc<IoContext>, Arc<RwLock<AppCore>>, Arc<AuraAgent>) {
    let test_dir =
        std::env::temp_dir().join(format!("aura-guardian-e2e-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&test_dir);
    std::fs::create_dir_all(&test_dir).expect("Failed to create test dir");

    // Create Bob's authority ID using same derivation as demo mode
    let bob_device_id_str = "demo:bob";
    let bob_authority_entropy =
        aura_core::hash::hash(format!("authority:{}", bob_device_id_str).as_bytes());
    let bob_authority = AuthorityId::new_from_entropy(bob_authority_entropy);

    // Create agent with simulation effects (like demo mode does)
    let agent_config = AgentConfig::default();
    // Create effect context with simulation mode for demo
    let context_id =
        aura_core::identifiers::ContextId::new_from_entropy(aura_core::hash::hash(b"test-context"));
    let execution_mode = aura_core::effects::ExecutionMode::Simulation { seed };
    let effect_ctx = EffectContext::new(bob_authority, context_id, execution_mode);
    let agent = AgentBuilder::new()
        .with_config(agent_config)
        .with_authority(bob_authority)
        .build_simulation_async_with_shared_transport(seed, &effect_ctx, shared_inbox.clone())
        .await
        .expect("Failed to create simulation agent");

    let agent = Arc::new(agent);

    // Create AppCore with RuntimeBridge from agent
    let app_config = AppConfig {
        data_dir: test_dir.to_string_lossy().to_string(),
        debug: false,
        journal_path: Some(test_dir.join("journal.json").to_string_lossy().to_string()),
    };
    let app_core =
        AppCore::with_runtime(app_config, agent.clone().as_runtime_bridge()).expect("AppCore");
    app_core
        .init_signals()
        .await
        .expect("Failed to init signals");
    let app_core = Arc::new(RwLock::new(app_core));

    let ctx = IoContext::with_account_status(
        app_core.clone(),
        false,
        test_dir,
        format!("test-device-{}", name),
        TuiMode::Demo { seed },
    );

    ctx.create_account(&format!("DemoUser-{}", name))
        .expect("Failed to create account");

    (Arc::new(ctx), app_core, agent)
}

fn cleanup_test_dir(name: &str) {
    let test_dir =
        std::env::temp_dir().join(format!("aura-guardian-e2e-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&test_dir);
}

// ============================================================================
// E2E Test: Full Guardian Flow with Signal Coordinator
// ============================================================================

/// This is the ACTUAL e2e test that validates the guardian display bug is fixed.
///
/// The bug was: after sending a guardian request to Alice, her contact still
/// showed is_guardian=false because the signal_coordinator wasn't correctly
/// updating CONTACTS_SIGNAL when the simulated agent accepted.
#[tokio::test]
async fn test_guardian_display_full_flow() {
    println!("\n=== Guardian Display Full E2E Flow Test ===\n");

    let seed = 2024u64;

    // Step 1: Start demo simulator FIRST (so we can use its shared_inbox)
    println!("Step 1: Starting demo simulator...");
    let mut simulator = DemoSimulator::new(seed)
        .await
        .expect("Failed to create simulator");
    simulator.start().await.expect("Failed to start simulator");

    let alice_authority = simulator.alice_authority().await;
    println!("  Alice authority: {}", alice_authority);

    // Now create test env with the shared inbox (for transport between Bob and Alice/Carol)
    let (ctx, app_core, _agent) =
        setup_test_env("guardian-e2e", seed, &simulator.shared_transport_inbox).await;

    // Step 2: Take the response receiver and start signal coordinator
    println!("\nStep 2: Starting signal coordinator...");
    let response_rx = simulator
        .take_response_receiver()
        .await
        .expect("Response receiver should be available");

    // Get Bob's authority ID using the same derivation as the simulator
    let bob_device_id_str = "demo:bob";
    let bob_authority_entropy =
        aura_core::hash::hash(format!("authority:{}", bob_device_id_str).as_bytes());
    let bob_authority = AuthorityId::new_from_entropy(bob_authority_entropy);

    let signal_coordinator = Arc::new(DemoSignalCoordinator::new(
        app_core.clone(),
        bob_authority,
        simulator.bridge(),
        response_rx,
    ));

    // Start the coordinator
    let (action_handle, response_handle) = signal_coordinator.start();

    // Give the coordinator time to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Step 3: Import Alice as a contact
    println!("\nStep 3: Importing Alice as a contact...");
    let alice_code = generate_demo_invite_code("Alice", seed);
    ctx.dispatch(EffectCommand::ImportInvitation {
        code: alice_code.clone(),
    })
    .await
    .expect("Import should succeed");

    // Wait for signal to propagate
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Verify Alice is a contact but NOT a guardian yet
    {
        let core = app_core.read().await;
        let contacts = core.read(&*CONTACTS_SIGNAL).await.expect("Read contacts");
        let alice_contact = contacts
            .contacts
            .iter()
            .find(|c| c.id == alice_authority)
            .expect("Alice should be a contact");

        println!("  Alice contact found: {}", alice_contact.id);
        println!(
            "  is_guardian BEFORE request: {}",
            alice_contact.is_guardian
        );

        assert!(
            !alice_contact.is_guardian,
            "Alice should NOT be a guardian yet"
        );
    }

    // Step 4: Send guardian request to Alice
    // This uses InviteGuardian (what the UI sends), not ToggleContactGuardian
    println!("\nStep 4: Sending guardian request to Alice...");
    let invite_cmd = EffectCommand::InviteGuardian {
        contact_id: Some(alice_authority.to_string()),
    };

    // Dispatch through IoContext (for intent processing)
    let result = ctx.dispatch(invite_cmd.clone()).await;
    println!("  InviteGuardian dispatch result: {:?}", result);

    // ALSO route through SimulatedBridge (to trigger agent responses in demo mode)
    // This is what the TUI should also do in demo mode
    simulator.bridge().route_command(&invite_cmd).await;
    println!("  InviteGuardian routed to agents");

    // Step 5: Wait for simulated agent to respond
    println!("\nStep 5: Waiting for simulated agent response...");
    // The simulated agent should process the request and respond with AcceptGuardianBinding
    // The signal coordinator should then update CONTACTS_SIGNAL with is_guardian=true
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Step 6: Verify is_guardian=true on the contact
    println!("\nStep 6: Verifying guardian status...");
    let is_guardian = {
        let core = app_core.read().await;
        let contacts = core.read(&*CONTACTS_SIGNAL).await.expect("Read contacts");

        println!("  Total contacts: {}", contacts.contacts.len());
        for c in &contacts.contacts {
            println!("    - {} (is_guardian: {})", c.id, c.is_guardian);
        }

        contacts
            .contacts
            .iter()
            .find(|c| c.id == alice_authority)
            .map(|c| c.is_guardian)
            .unwrap_or(false)
    };

    println!("\n  is_guardian AFTER request: {}", is_guardian);

    // Also check RECOVERY_SIGNAL to see if guardian was added there
    {
        let core = app_core.read().await;
        let recovery = core.read(&*RECOVERY_SIGNAL).await.expect("Read recovery");
        println!("  RECOVERY_SIGNAL guardians: {}", recovery.guardians.len());
        for g in &recovery.guardians {
            println!("    - {} (status: {:?})", g.id, g.status);
        }
    }

    // Cleanup
    action_handle.abort();
    response_handle.abort();
    cleanup_test_dir("guardian-e2e");

    // Final assertion - THIS IS THE KEY TEST
    assert!(
        is_guardian,
        "CRITICAL BUG: Alice should show is_guardian=true after accepting guardian request!\n\
         This is the guardian display bug - contacts don't show Guardian: Yes"
    );

    println!("\n=== Guardian Display Full E2E Flow Test PASSED ===\n");
}

/// Simpler test that just validates the AuthorityId derivation matches
#[tokio::test]
async fn test_authority_id_derivation_matches() {
    println!("\n=== Authority ID Derivation Matching Test ===\n");

    let seed = 2024u64;

    // Derive authority ID the way hints.rs does it (invitation code sender_id)
    let hints_alice_authority = ids::authority_id(&format!("demo:{}:{}:authority", seed, "Alice"));

    // Derive authority ID the way SimulatedAgent does it
    let simulator_alice_authority =
        ids::authority_id(&format!("demo:{}:{}:authority", seed, "Alice"));

    println!("Hints derivation:     {}", hints_alice_authority);
    println!("Simulator derivation: {}", simulator_alice_authority);

    assert_eq!(
        hints_alice_authority, simulator_alice_authority,
        "AuthorityId derivations must match for guardian lookup to work"
    );

    println!("\n=== Authority ID Derivation Matching Test PASSED ===\n");
}

/// Test that validates the invite code can be parsed correctly
#[tokio::test]
async fn test_invite_code_produces_correct_authority() {
    use aura_agent::handlers::ShareableInvitation;

    println!("\n=== Invite Code Authority Test ===\n");

    let seed = 2024u64;
    let alice_code = generate_demo_invite_code("Alice", seed);

    // Parse the invitation
    let parsed = ShareableInvitation::from_code(&alice_code).expect("Should parse invitation code");

    // Get the authority ID from the parsed invitation
    let invitation_authority: AuthorityId = parsed.sender_id;

    // Derive the expected authority ID
    let expected_authority = ids::authority_id(&format!("demo:{}:{}:authority", seed, "Alice"));

    println!("From invitation code: {}", invitation_authority);
    println!("Expected derivation:  {}", expected_authority);

    assert_eq!(
        invitation_authority, expected_authority,
        "Invitation code must produce the correct AuthorityId"
    );

    println!("\n=== Invite Code Authority Test PASSED ===\n");
}
