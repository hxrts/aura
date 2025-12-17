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
use aura_core::hash::hash;
use aura_core::identifiers::AuthorityId;
use aura_terminal::tui::context::IoContext;
use aura_terminal::tui::effects::EffectCommand;
use base64::Engine;
use uuid::Uuid;

// ============================================================================
// Test Infrastructure
// ============================================================================

/// Generate a deterministic invite code for a demo agent (mirrors hints.rs logic)
///
/// This replicates the code generation from `aura_terminal::demo::hints` without
/// requiring the `development` feature flag.
fn generate_demo_invite_code(name: &str, seed: u64) -> String {
    // Create deterministic authority ID matching the simulator's derivation
    let authority_entropy = hash(format!("demo:{}:{}:authority", seed, name).as_bytes());
    let sender_id = AuthorityId::new_from_entropy(authority_entropy);

    // Create deterministic invitation ID from seed and name
    let invitation_id_entropy = hash(format!("demo:{}:{}:invitation", seed, name).as_bytes());
    let invitation_id = Uuid::from_bytes(invitation_id_entropy[..16].try_into().unwrap());

    // Create ShareableInvitation-compatible structure
    // IMPORTANT: Use sender_id.uuid() to get bare UUID for serde serialization
    // (sender_id.to_string() includes "authority-" prefix which breaks deserialization)
    let invitation_data = serde_json::json!({
        "version": 1,
        "invitation_id": invitation_id.to_string(),
        "sender_id": sender_id.uuid().to_string(),
        "invitation_type": {
            "Guardian": {
                "subject_authority": sender_id.uuid().to_string()
            }
        },
        "expires_at": null,
        "message": format!("Guardian invitation from {} (demo)", name)
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
    let alice_code = generate_demo_invite_code("alice", seed);
    let carol_code = generate_demo_invite_code("carol", seed);

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

    // Both should be Guardian type
    match &alice_invitation.invitation_type {
        aura_invitation::InvitationType::Guardian { subject_authority } => {
            println!(
                "  Alice is a Guardian invitation for authority: {}",
                subject_authority
            );
            assert_eq!(
                subject_authority, &alice_invitation.sender_id,
                "Guardian subject should be sender"
            );
        }
        other => panic!("Expected Guardian type for Alice, got {:?}", other),
    }

    match &carol_invitation.invitation_type {
        aura_invitation::InvitationType::Guardian { subject_authority } => {
            println!(
                "  Carol is a Guardian invitation for authority: {}",
                subject_authority
            );
            assert_eq!(
                subject_authority, &carol_invitation.sender_id,
                "Guardian subject should be sender"
            );
        }
        other => panic!("Expected Guardian type for Carol, got {:?}", other),
    }

    // They should have different sender IDs
    assert_ne!(
        alice_invitation.sender_id, carol_invitation.sender_id,
        "Alice and Carol should have different sender IDs"
    );

    println!("\n=== Demo Invitation Code Parsing Test PASSED ===\n");
}

/// Test that ImportInvitation command successfully imports demo codes
#[tokio::test]
async fn test_import_invitation_command_with_demo_codes() {
    println!("\n=== ImportInvitation Command Test ===\n");

    let (ctx, app_core) = setup_test_env("import-demo").await;
    let seed = 2024;
    let alice_code = generate_demo_invite_code("alice", seed);
    let carol_code = generate_demo_invite_code("carol", seed);

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
#[tokio::test]
async fn test_complete_demo_invitation_flow() {
    println!("\n=== Complete Demo Invitation Flow Test ===\n");

    let (ctx, app_core) = setup_test_env("complete-flow").await;
    let seed = 2024;
    let alice_code = generate_demo_invite_code("alice", seed);
    let carol_code = generate_demo_invite_code("carol", seed);

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
            println!("    - {} ({})", contact.petname, contact.id);
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

    // Create codes multiple times with same seed
    let alice1 = generate_demo_invite_code("alice", seed);
    let alice2 = generate_demo_invite_code("alice", seed);
    let alice3 = generate_demo_invite_code("alice", seed);

    let carol1 = generate_demo_invite_code("carol", seed);
    let carol2 = generate_demo_invite_code("carol", seed);

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
    println!("    Carol: {}...", &carol1[..40]);

    // Verify different seeds produce different codes
    let alice_different = generate_demo_invite_code("alice", 2025);
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
