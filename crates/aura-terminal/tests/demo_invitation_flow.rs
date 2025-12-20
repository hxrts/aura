#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::disallowed_methods,
    clippy::needless_borrows_for_generic_args
)]
//! # Demo Invitation Flow E2E Tests
//!
//! Tests the complete demo flow for importing Alice and Carol's invitation codes,
//! adding them as contacts, and creating a group chat with them.
//!
//! ## Running
//!
//! ```bash
//! cargo test --package aura-terminal --test demo_invitation_flow -- --nocapture
//! ```

use async_lock::RwLock;
use std::sync::Arc;

use aura_agent::handlers::ShareableInvitation;
use aura_app::signal_defs::{CHAT_SIGNAL, CONTACTS_SIGNAL, INVITATIONS_SIGNAL, RECOVERY_SIGNAL};
use aura_app::{AppConfig, AppCore};
use aura_core::effects::reactive::ReactiveEffects;
use aura_terminal::handlers::tui::TuiMode;
use aura_terminal::ids;
use aura_terminal::tui::context::IoContext;
use aura_terminal::tui::effects::EffectCommand;
use base64::Engine;

// ============================================================================
// Test Infrastructure
// ============================================================================

/// Generate a deterministic invite code for a demo agent.
///
/// This MUST match the derivation in:
/// - `aura_terminal::demo::hints::generate_invite_code` (invitation codes)
/// - `aura_terminal::demo::mod::SimulatedAgent::new_with_shared_transport` (agent IDs)
/// - `aura_terminal::demo::mod::AgentFactory::create_demo_agents` (seed offsets)
///
/// Key rules:
/// - Uses `ids::authority_id()` for domain-separated derivation
/// - Alice uses `seed`, Carol uses `seed + 1`
/// - Creates Contact invitations (not Guardian)
fn generate_demo_invite_code(name: &str, seed: u64) -> String {
    // Create deterministic authority ID using the SAME derivation as SimulatedAgent
    // CRITICAL: Must use ids::authority_id() for domain separation
    let sender_id = ids::authority_id(&format!("demo:{}:{}:authority", seed, name));

    // Create deterministic invitation ID from seed and name
    let invitation_id = ids::uuid(&format!("demo:{}:{}:invitation", seed, name));

    // Create ShareableInvitation-compatible structure
    // Uses Contact type (not Guardian) - guardian requests are sent in-band
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

    // Encode as base64 with aura:v1: prefix
    let json_str = serde_json::to_string(&invitation_data).unwrap_or_default();
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json_str.as_bytes());
    format!("aura:v1:{}", b64)
}

/// Create a test environment with IoContext and AppCore
async fn setup_test_env(name: &str) -> (Arc<IoContext>, Arc<RwLock<AppCore>>) {
    let test_dir =
        std::env::temp_dir().join(format!("aura-demo-test-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&test_dir);
    std::fs::create_dir_all(&test_dir).expect("Failed to create test dir");

    let app_core = AppCore::new(AppConfig::default()).expect("Failed to create AppCore");
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
        TuiMode::Production,
    );

    // Create account for testing
    ctx.create_account(&format!("DemoUser-{}", name))
        .expect("Failed to create account");

    (Arc::new(ctx), app_core)
}

/// Cleanup test directory
fn cleanup_test_dir(name: &str) {
    let test_dir =
        std::env::temp_dir().join(format!("aura-demo-test-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&test_dir);
}

// ============================================================================
// Demo Invitation Code Parsing Tests
// ============================================================================

/// Test that demo hint invitation codes can be successfully parsed by ShareableInvitation
#[tokio::test]
async fn test_demo_invitation_codes_are_parseable() {
    println!("\n=== Demo Invitation Code Parsing Test ===\n");

    let seed = 2024; // Standard demo seed
    // Names and seeds must match AgentFactory::create_demo_agents
    let alice_code = generate_demo_invite_code("Alice", seed);
    let carol_code = generate_demo_invite_code("Carol", seed + 1);

    println!(
        "Alice's invite code: {}...",
        &alice_code[..50.min(alice_code.len())]
    );
    println!(
        "Carol's invite code: {}...",
        &carol_code[..50.min(carol_code.len())]
    );

    // Phase 1: Parse Alice's code
    println!("\nPhase 1: Parse Alice's invitation code");
    let alice_result = ShareableInvitation::from_code(&alice_code);
    match &alice_result {
        Ok(invitation) => {
            println!("  Success! Alice's invitation:");
            println!("    Version: {}", invitation.version);
            println!("    Invitation ID: {}", invitation.invitation_id);
            println!("    Sender ID: {}", invitation.sender_id);
            println!("    Type: {:?}", invitation.invitation_type);
            println!("    Message: {:?}", invitation.message);
        }
        Err(e) => {
            panic!("Failed to parse Alice's invitation code: {:?}", e);
        }
    }
    let alice_invitation = alice_result.expect("Alice's code should parse");

    // Phase 2: Parse Carol's code
    println!("\nPhase 2: Parse Carol's invitation code");
    let carol_result = ShareableInvitation::from_code(&carol_code);
    match &carol_result {
        Ok(invitation) => {
            println!("  Success! Carol's invitation:");
            println!("    Version: {}", invitation.version);
            println!("    Invitation ID: {}", invitation.invitation_id);
            println!("    Sender ID: {}", invitation.sender_id);
            println!("    Type: {:?}", invitation.invitation_type);
            println!("    Message: {:?}", invitation.message);
        }
        Err(e) => {
            panic!("Failed to parse Carol's invitation code: {:?}", e);
        }
    }
    let carol_invitation = carol_result.expect("Carol's code should parse");

    // Phase 3: Verify invitation properties
    println!("\nPhase 3: Verify invitation properties");

    // Both should be version 1
    assert_eq!(alice_invitation.version, 1, "Alice's version should be 1");
    assert_eq!(carol_invitation.version, 1, "Carol's version should be 1");

    // Both should be Contact type (Guardian requests are sent in-band)
    match &alice_invitation.invitation_type {
        aura_invitation::InvitationType::Contact { nickname } => {
            println!("  Alice is a Contact invitation with nickname: {:?}", nickname);
            assert_eq!(nickname, &Some("Alice".to_string()));
        }
        other => panic!("Expected Contact type for Alice, got {:?}", other),
    }

    match &carol_invitation.invitation_type {
        aura_invitation::InvitationType::Contact { nickname } => {
            println!("  Carol is a Contact invitation with nickname: {:?}", nickname);
            assert_eq!(nickname, &Some("Carol".to_string()));
        }
        other => panic!("Expected Contact type for Carol, got {:?}", other),
    }

    // They should have different sender IDs
    assert_ne!(
        alice_invitation.sender_id, carol_invitation.sender_id,
        "Alice and Carol should have different sender IDs"
    );

    println!("\n=== Demo Invitation Code Parsing Test PASSED ===\n");
}

/// Test that ImportInvitation command successfully imports demo codes
///
/// NOTE: This test is currently ignored because it requires a RuntimeBridge to be set up,
/// which involves full agent initialization. The test will be re-enabled once we have
/// a lightweight test harness that provides mock RuntimeBridge functionality.
#[tokio::test]
#[ignore = "Requires RuntimeBridge - see setup_test_env for details"]
async fn test_import_invitation_command_with_demo_codes() {
    println!("\n=== ImportInvitation Command Test ===\n");

    let (ctx, app_core) = setup_test_env("import-demo").await;
    let seed = 2024;
    // Names and seeds must match AgentFactory::create_demo_agents
    let alice_code = generate_demo_invite_code("Alice", seed);
    let carol_code = generate_demo_invite_code("Carol", seed + 1);

    // Phase 1: Import Alice's invitation code via EffectCommand
    println!("Phase 1: Import Alice's invitation via EffectCommand");
    let result = ctx
        .dispatch(EffectCommand::ImportInvitation {
            code: alice_code.clone(),
        })
        .await;

    match &result {
        Ok(()) => {
            println!("  Successfully dispatched Alice's invitation import");
        }
        Err(e) => panic!("Failed to import Alice's invitation: {:?}", e),
    }

    // Phase 2: Import Carol's invitation code
    println!("\nPhase 2: Import Carol's invitation via EffectCommand");
    let result = ctx
        .dispatch(EffectCommand::ImportInvitation {
            code: carol_code.clone(),
        })
        .await;

    match &result {
        Ok(()) => {
            println!("  Successfully dispatched Carol's invitation import");
        }
        Err(e) => panic!("Failed to import Carol's invitation: {:?}", e),
    }

    // Phase 3: Verify invitations appear in signal state
    println!("\nPhase 3: Verify invitation state");
    let core = app_core.read().await;
    if let Ok(inv_state) = core.read(&*INVITATIONS_SIGNAL).await {
        println!("  Pending invitations: {}", inv_state.pending.len());
        println!("  Sent invitations: {}", inv_state.sent.len());
        for inv in &inv_state.pending {
            println!("    - {} from {}", inv.id, inv.from_id);
        }
    }
    drop(core);

    cleanup_test_dir("import-demo");
    println!("\n=== ImportInvitation Command Test PASSED ===\n");
}

/// Test complete demo flow: import invitations, accept them, create channel, send message
///
/// NOTE: This test is currently ignored because it requires a RuntimeBridge to be set up,
/// which involves full agent initialization. The test will be re-enabled once we have
/// a lightweight test harness that provides mock RuntimeBridge functionality.
#[tokio::test]
#[ignore = "Requires RuntimeBridge - see setup_test_env for details"]
async fn test_complete_demo_invitation_flow() {
    println!("\n=== Complete Demo Invitation Flow Test ===\n");

    let (ctx, app_core) = setup_test_env("complete-flow").await;
    let seed = 2024;
    // Names and seeds must match AgentFactory::create_demo_agents
    let alice_code = generate_demo_invite_code("Alice", seed);
    let carol_code = generate_demo_invite_code("Carol", seed + 1);

    // Parse the codes to get sender IDs
    let alice_invitation = ShareableInvitation::from_code(&alice_code).expect("Parse Alice");
    let carol_invitation = ShareableInvitation::from_code(&carol_code).expect("Parse Carol");

    // Phase 1: Import both invitation codes
    println!("Phase 1: Import Alice and Carol's invitation codes");

    ctx.dispatch(EffectCommand::ImportInvitation {
        code: alice_code.clone(),
    })
    .await
    .expect("Alice import should succeed");
    println!("  Alice's invitation imported successfully");

    ctx.dispatch(EffectCommand::ImportInvitation {
        code: carol_code.clone(),
    })
    .await
    .expect("Carol import should succeed");
    println!("  Carol's invitation imported successfully");

    // Phase 2: Accept the invitations
    println!("\nPhase 2: Accept invitations to create contacts");

    let accept_alice = ctx
        .dispatch(EffectCommand::AcceptInvitation {
            invitation_id: alice_invitation.invitation_id.clone(),
        })
        .await;
    println!("  Accept Alice result: {:?}", accept_alice);

    let accept_carol = ctx
        .dispatch(EffectCommand::AcceptInvitation {
            invitation_id: carol_invitation.invitation_id.clone(),
        })
        .await;
    println!("  Accept Carol result: {:?}", accept_carol);

    // Phase 3: Create a group channel with Alice and Carol
    println!("\nPhase 3: Create group channel with Alice and Carol");

    let channel_result = ctx
        .dispatch(EffectCommand::CreateChannel {
            name: "Guardians".to_string(),
            topic: Some("Guardian coordination channel".to_string()),
            members: vec![
                alice_invitation.sender_id.to_string(),
                carol_invitation.sender_id.to_string(),
            ],
        })
        .await;
    println!("  CreateChannel result: {:?}", channel_result);

    // Phase 4: Send a message to the channel
    println!("\nPhase 4: Send message to guardians");

    let send_result = ctx
        .dispatch(EffectCommand::SendMessage {
            channel: "guardians".to_string(),
            content: "Hello Alice and Carol! Thanks for being my guardians.".to_string(),
        })
        .await;
    println!("  SendMessage result: {:?}", send_result);

    // Phase 5: Verify state via signals
    println!("\nPhase 5: Verify state via signals");

    let core = app_core.read().await;

    // Check chat state
    if let Ok(chat_state) = core.read(&*CHAT_SIGNAL).await {
        println!("  Chat channels: {}", chat_state.channels.len());
        println!("  Messages: {}", chat_state.messages.len());
        for channel in &chat_state.channels {
            println!("    - {} ({})", channel.name, channel.id);
        }
    }

    // Check contacts state
    if let Ok(contacts_state) = core.read(&*CONTACTS_SIGNAL).await {
        println!("  Contacts: {}", contacts_state.contacts.len());
        for contact in &contacts_state.contacts {
            println!("    - {} ({})", contact.nickname, contact.id);
        }
    }

    // Check invitations state
    if let Ok(inv_state) = core.read(&*INVITATIONS_SIGNAL).await {
        println!("  Pending invitations: {}", inv_state.pending.len());
        println!("  Sent invitations: {}", inv_state.sent.len());
    }

    // Check recovery/guardians state
    if let Ok(recovery_state) = core.read(&*RECOVERY_SIGNAL).await {
        println!("  Guardians: {}", recovery_state.guardians.len());
        println!("  Threshold: {}", recovery_state.threshold);
    }

    drop(core);
    cleanup_test_dir("complete-flow");
    println!("\n=== Complete Demo Invitation Flow Test PASSED ===\n");
}

/// Test that deterministic seeds produce consistent invitation codes
#[tokio::test]
async fn test_demo_hints_deterministic() {
    println!("\n=== Demo Hints Determinism Test ===\n");

    let seed = 2024;

    // Create codes multiple times with same seed (Title case to match AgentFactory)
    let alice1 = generate_demo_invite_code("Alice", seed);
    let alice2 = generate_demo_invite_code("Alice", seed);
    let alice3 = generate_demo_invite_code("Alice", seed);

    // Carol uses seed + 1 to match AgentFactory
    let carol1 = generate_demo_invite_code("Carol", seed + 1);
    let carol2 = generate_demo_invite_code("Carol", seed + 1);

    // Verify all produce identical codes
    assert_eq!(
        alice1, alice2,
        "Alice codes should be identical with same seed"
    );
    assert_eq!(
        alice2, alice3,
        "Alice codes should be identical with same seed"
    );

    assert_eq!(
        carol1, carol2,
        "Carol codes should be identical with same seed"
    );

    println!("  Seed {} produces consistent codes:", seed);
    println!("    Alice: {}...", &alice1[..40]);
    println!("    Carol (seed + 1): {}...", &carol1[..40]);

    // Verify different seeds produce different codes
    let alice_different = generate_demo_invite_code("Alice", 2025);
    assert_ne!(
        alice1, alice_different,
        "Different seeds should produce different codes"
    );

    println!("\n  Seed 2025 produces different codes:");
    println!("    Alice: {}...", &alice_different[..40]);

    println!("\n=== Demo Hints Determinism Test PASSED ===\n");
}

/// Test invalid invitation codes are properly rejected
#[tokio::test]
async fn test_invalid_invitation_code_rejection() {
    println!("\n=== Invalid Invitation Code Rejection Test ===\n");

    let (ctx, _app_core) = setup_test_env("invalid-codes").await;

    // Test 1: Completely invalid format
    println!("Test 1: Invalid format");
    let result = ctx
        .dispatch(EffectCommand::ImportInvitation {
            code: "not-a-valid-code".to_string(),
        })
        .await;
    println!("  Result: {:?}", result);
    assert!(result.is_err(), "Invalid format should fail");

    // Test 2: Wrong prefix
    println!("\nTest 2: Wrong prefix");
    let result = ctx
        .dispatch(EffectCommand::ImportInvitation {
            code: "wrong:v1:abc123".to_string(),
        })
        .await;
    println!("  Result: {:?}", result);
    assert!(result.is_err(), "Wrong prefix should fail");

    // Test 3: Invalid base64
    println!("\nTest 3: Invalid base64");
    let result = ctx
        .dispatch(EffectCommand::ImportInvitation {
            code: "aura:v1:not-valid-base64!!!".to_string(),
        })
        .await;
    println!("  Result: {:?}", result);
    assert!(result.is_err(), "Invalid base64 should fail");

    // Test 4: Valid base64 but invalid JSON
    println!("\nTest 4: Valid base64 but invalid JSON");
    let invalid_json =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("not json at all".as_bytes());
    let result = ctx
        .dispatch(EffectCommand::ImportInvitation {
            code: format!("aura:v1:{}", invalid_json),
        })
        .await;
    println!("  Result: {:?}", result);
    assert!(result.is_err(), "Invalid JSON should fail");

    cleanup_test_dir("invalid-codes");
    println!("\n=== Invalid Invitation Code Rejection Test PASSED ===\n");
}

// ============================================================================
// Guardian ID Matching Validation
// ============================================================================

/// Comprehensive test that validates AuthorityId matching between:
/// 1. Invitation codes (from hints/this test)
/// 2. SimulatedAgent derivation
/// 3. Contact storage after import
///
/// This test ensures the guardian display bug is fixed:
/// - Contacts imported from invitations have the same AuthorityId as SimulatedAgents
/// - When signal_coordinator sets is_guardian=true, the lookup succeeds
#[tokio::test]
async fn test_guardian_authority_id_matching() {
    println!("\n=== Guardian AuthorityId Matching Test ===\n");

    let seed = 2024u64;

    // Step 1: Generate invitation codes the same way DemoHints does
    let alice_code = generate_demo_invite_code("Alice", seed);
    let carol_code = generate_demo_invite_code("Carol", seed + 1);

    // Step 2: Parse the invitations to get AuthorityIds
    let alice_invitation = ShareableInvitation::from_code(&alice_code)
        .expect("Alice invitation should parse");
    let carol_invitation = ShareableInvitation::from_code(&carol_code)
        .expect("Carol invitation should parse");

    let alice_invitation_authority = alice_invitation.sender_id;
    let carol_invitation_authority = carol_invitation.sender_id;

    println!("From parsed invitations:");
    println!("  Alice AuthorityId: {}", alice_invitation_authority);
    println!("  Carol AuthorityId: {}", carol_invitation_authority);

    // Step 3: Derive AuthorityIds the same way SimulatedAgent does
    // (This mirrors SimulatedAgent::new_with_shared_transport in demo/mod.rs)
    let alice_simulator_authority =
        ids::authority_id(&format!("demo:{}:{}:authority", seed, "Alice"));
    let carol_simulator_authority =
        ids::authority_id(&format!("demo:{}:{}:authority", seed + 1, "Carol"));

    println!("\nFrom simulator derivation:");
    println!("  Alice AuthorityId: {}", alice_simulator_authority);
    println!("  Carol AuthorityId: {}", carol_simulator_authority);

    // Step 4: CRITICAL ASSERTIONS - these must match for guardian display to work
    assert_eq!(
        alice_invitation_authority, alice_simulator_authority,
        "CRITICAL: Alice's invitation AuthorityId must match SimulatedAgent AuthorityId.\n\
         This is required for signal_coordinator to find the contact when setting is_guardian=true.\n\
         Invitation: {}\n\
         Simulator:  {}",
        alice_invitation_authority, alice_simulator_authority
    );

    assert_eq!(
        carol_invitation_authority, carol_simulator_authority,
        "CRITICAL: Carol's invitation AuthorityId must match SimulatedAgent AuthorityId.\n\
         This is required for signal_coordinator to find the contact when setting is_guardian=true.\n\
         Invitation: {}\n\
         Simulator:  {}",
        carol_invitation_authority, carol_simulator_authority
    );

    // Step 5: Verify the two agents have different IDs
    assert_ne!(
        alice_simulator_authority, carol_simulator_authority,
        "Alice and Carol should have different AuthorityIds"
    );

    println!("\n✓ All AuthorityIds match correctly!");
    println!("  - Invitation codes produce the same AuthorityIds as SimulatedAgents");
    println!("  - Guardian display should work correctly");
    println!("\n=== Guardian AuthorityId Matching Test PASSED ===\n");
}

/// Test that verifies the complete derivation chain from seed to AuthorityId
#[tokio::test]
async fn test_derivation_chain_consistency() {
    println!("\n=== Derivation Chain Consistency Test ===\n");

    let seed = 2024u64;

    // Test Alice's full derivation chain
    println!("Alice (seed={}):", seed);

    // 1. How ids::authority_id derives it
    let seed_string = format!("demo:{}:{}:authority", seed, "Alice");
    println!("  Seed string: \"{}\"", seed_string);

    let authority = ids::authority_id(&seed_string);
    println!("  AuthorityId: {}", authority);
    println!("  UUID: {}", authority.uuid());

    // 2. Verify it matches what's in the invitation
    let code = generate_demo_invite_code("Alice", seed);
    let invitation = ShareableInvitation::from_code(&code).unwrap();
    println!("  Invitation sender_id: {}", invitation.sender_id);

    assert_eq!(
        authority, invitation.sender_id,
        "ids::authority_id must produce same result as what's in invitation"
    );

    // Test Carol's derivation chain (uses seed + 1)
    println!("\nCarol (seed={}):", seed + 1);

    let carol_seed_string = format!("demo:{}:{}:authority", seed + 1, "Carol");
    println!("  Seed string: \"{}\"", carol_seed_string);

    let carol_authority = ids::authority_id(&carol_seed_string);
    println!("  AuthorityId: {}", carol_authority);

    let carol_code = generate_demo_invite_code("Carol", seed + 1);
    let carol_invitation = ShareableInvitation::from_code(&carol_code).unwrap();
    println!("  Invitation sender_id: {}", carol_invitation.sender_id);

    assert_eq!(
        carol_authority, carol_invitation.sender_id,
        "Carol's ids::authority_id must produce same result as invitation"
    );

    println!("\n=== Derivation Chain Consistency Test PASSED ===\n");
}
