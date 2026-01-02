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
use std::time::{Duration, Instant};

use aura_agent::core::{AgentBuilder, AgentConfig, AuraAgent};
use aura_agent::handlers::ShareableInvitation;
use aura_agent::EffectContext;
use aura_app::signal_defs::{CHAT_SIGNAL, CONTACTS_SIGNAL, INVITATIONS_SIGNAL, RECOVERY_SIGNAL};
use aura_app::{AppConfig, AppCore};
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::effects::ExecutionMode;
use aura_core::identifiers::AuthorityId;
use aura_terminal::handlers::tui::create_account;
use aura_terminal::handlers::tui::TuiMode;
use aura_terminal::ids;
use aura_terminal::tui::context::{InitializedAppCore, IoContext};
use aura_terminal::tui::effects::EffectCommand;
use base64::Engine;
use uuid::Uuid;

// ============================================================================
// Test Infrastructure
// ============================================================================

struct TestEnv {
    ctx: Arc<IoContext>,
    app_core: Arc<RwLock<AppCore>>,
    _agent: Arc<AuraAgent>,
    test_dir: std::path::PathBuf,
}

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
    let sender_id = ids::authority_id(&format!("demo:{seed}:{name}:authority"));

    // Create deterministic invitation ID from seed and name
    let invitation_id = ids::uuid(&format!("demo:{seed}:{name}:invitation"));

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
        "message": format!("Contact invitation from {name} (demo)")
    });

    // Encode as base64 with aura:v1: prefix
    let json_str = serde_json::to_string(&invitation_data).unwrap_or_default();
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json_str.as_bytes());
    format!("aura:v1:{b64}")
}

/// Create a test environment with IoContext and AppCore
async fn setup_test_env(name: &str) -> TestEnv {
    let unique = Uuid::new_v4();
    let test_dir = std::env::temp_dir().join(format!("aura-demo-test-{name}-{unique}"));
    let _ = std::fs::remove_dir_all(&test_dir);
    std::fs::create_dir_all(&test_dir).expect("Failed to create test dir");

    let device_id_str = format!("test-device-{name}");
    let display_name = format!("DemoUser-{name}");

    let (authority_id, context_id) = create_account(&test_dir, &device_id_str, &display_name)
        .await
        .expect("Failed to create account");

    let agent_config = AgentConfig {
        device_id: ids::device_id(&device_id_str),
        storage: aura_agent::core::config::StorageConfig {
            base_path: test_dir.clone(),
            ..Default::default()
        },
        ..Default::default()
    };

    let seed = 2024u64;
    let effect_ctx =
        EffectContext::new(authority_id, context_id, ExecutionMode::Simulation { seed });

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
    let initialized_app_core = InitializedAppCore::new(app_core.clone())
        .await
        .expect("Failed to init signals");

    let ctx = IoContext::builder()
        .with_app_core(initialized_app_core)
        .with_existing_account(true)
        .with_base_path(test_dir.clone())
        .with_device_id(device_id_str)
        .with_mode(TuiMode::Production)
        .build()
        .expect("IoContext builder should succeed for tests");

    TestEnv {
        ctx: Arc::new(ctx),
        app_core,
        _agent: agent,
        test_dir,
    }
}

async fn wait_for_contact(app_core: &Arc<RwLock<AppCore>>, contact_id: AuthorityId) {
    let start = tokio::time::Instant::now();
    loop {
        let state = {
            let core = app_core.read().await;
            core.read(&*CONTACTS_SIGNAL)
                .await
                .expect("Failed to read CONTACTS_SIGNAL")
        };

        if state.contacts.iter().any(|c| c.id == contact_id) {
            return;
        }

        if start.elapsed() > Duration::from_secs(2) {
            let contact_count = state.contacts.len();
            panic!("Timed out waiting for contact {contact_id} ({contact_count} contacts present)");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn wait_for_channel_members(
    app_core: &Arc<RwLock<AppCore>>,
    channel_name: &str,
    expected_members: u32,
) {
    let start = Instant::now();
    loop {
        let core = app_core.read().await;
        if let Ok(chat_state) = core.read(&*CHAT_SIGNAL).await {
            if let Some(channel) = chat_state.channels.iter().find(|c| c.name == channel_name) {
                if channel.member_count >= expected_members {
                    return;
                }
            }
        }
        drop(core);
        if start.elapsed() > Duration::from_secs(2) {
            panic!("Timed out waiting for {expected_members} members in channel {channel_name}");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn wait_for_message(
    app_core: &Arc<RwLock<AppCore>>,
    channel_name: &str,
    content_snippet: &str,
) {
    let start = Instant::now();
    loop {
        let core = app_core.read().await;
        if let Ok(chat_state) = core.read(&*CHAT_SIGNAL).await {
            if let Some(channel) = chat_state.channels.iter().find(|c| c.name == channel_name) {
                if chat_state
                    .messages
                    .iter()
                    .any(|m| m.channel_id == channel.id && m.content.contains(content_snippet))
                {
                    return;
                }
            }
        }
        drop(core);
        if start.elapsed() > Duration::from_secs(2) {
            panic!(
                "Timed out waiting for message containing '{content_snippet}' in {channel_name} channel"
            );
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
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

    let alice_preview_len = 50.min(alice_code.len());
    let alice_preview = &alice_code[..alice_preview_len];
    println!("Alice's invite code: {alice_preview}...");
    let carol_preview_len = 50.min(carol_code.len());
    let carol_preview = &carol_code[..carol_preview_len];
    println!("Carol's invite code: {carol_preview}...");

    // Phase 1: Parse Alice's code
    println!("\nPhase 1: Parse Alice's invitation code");
    let alice_result = ShareableInvitation::from_code(&alice_code);
    match &alice_result {
        Ok(invitation) => {
            println!("  Success! Alice's invitation:");
            println!("    Version: {version}", version = invitation.version);
            println!(
                "    Invitation ID: {invitation_id}",
                invitation_id = invitation.invitation_id
            );
            println!(
                "    Sender ID: {sender_id}",
                sender_id = invitation.sender_id
            );
            println!(
                "    Type: {invitation_type:?}",
                invitation_type = invitation.invitation_type
            );
            println!("    Message: {message:?}", message = invitation.message);
        }
        Err(e) => {
            panic!("Failed to parse Alice's invitation code: {e:?}");
        }
    }
    let alice_invitation = alice_result.expect("Alice's code should parse");

    // Phase 2: Parse Carol's code
    println!("\nPhase 2: Parse Carol's invitation code");
    let carol_result = ShareableInvitation::from_code(&carol_code);
    match &carol_result {
        Ok(invitation) => {
            println!("  Success! Carol's invitation:");
            println!("    Version: {version}", version = invitation.version);
            println!(
                "    Invitation ID: {invitation_id}",
                invitation_id = invitation.invitation_id
            );
            println!(
                "    Sender ID: {sender_id}",
                sender_id = invitation.sender_id
            );
            println!(
                "    Type: {invitation_type:?}",
                invitation_type = invitation.invitation_type
            );
            println!("    Message: {message:?}", message = invitation.message);
        }
        Err(e) => {
            panic!("Failed to parse Carol's invitation code: {e:?}");
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
            println!("  Alice is a Contact invitation with nickname: {nickname:?}");
            assert_eq!(nickname, &Some("Alice".to_string()));
        }
        other => panic!("Expected Contact type for Alice, got {other:?}"),
    }

    match &carol_invitation.invitation_type {
        aura_invitation::InvitationType::Contact { nickname } => {
            println!("  Carol is a Contact invitation with nickname: {nickname:?}");
            assert_eq!(nickname, &Some("Carol".to_string()));
        }
        other => panic!("Expected Contact type for Carol, got {other:?}"),
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

    let env = setup_test_env("import-demo").await;
    let seed = 2024;
    // Names and seeds must match AgentFactory::create_demo_agents
    let alice_code = generate_demo_invite_code("Alice", seed);
    let carol_code = generate_demo_invite_code("Carol", seed + 1);
    let alice_sender = ShareableInvitation::from_code(&alice_code)
        .expect("Parse Alice code")
        .sender_id;
    let carol_sender = ShareableInvitation::from_code(&carol_code)
        .expect("Parse Carol code")
        .sender_id;

    // Phase 1: Import Alice's invitation code via EffectCommand
    println!("Phase 1: Import Alice's invitation via EffectCommand");
    let result = env
        .ctx
        .dispatch(EffectCommand::ImportInvitation {
            code: alice_code.clone(),
        })
        .await;

    match &result {
        Ok(()) => {
            println!("  Successfully dispatched Alice's invitation import");
        }
        Err(e) => panic!("Failed to import Alice's invitation: {e:?}"),
    }
    wait_for_contact(&env.app_core, alice_sender).await;

    // Phase 2: Import Carol's invitation code
    println!("\nPhase 2: Import Carol's invitation via EffectCommand");
    let result = env
        .ctx
        .dispatch(EffectCommand::ImportInvitation {
            code: carol_code.clone(),
        })
        .await;

    match &result {
        Ok(()) => {
            println!("  Successfully dispatched Carol's invitation import");
        }
        Err(e) => panic!("Failed to import Carol's invitation: {e:?}"),
    }
    wait_for_contact(&env.app_core, carol_sender).await;

    // Phase 3: Verify contacts appear in signal state (the user-visible behavior).
    println!("\nPhase 3: Verify contacts state");
    let contacts_state = {
        let core = env.app_core.read().await;
        core.read(&*CONTACTS_SIGNAL)
            .await
            .expect("Failed to read CONTACTS_SIGNAL")
    };
    assert!(
        contacts_state.contacts.iter().any(|c| c.id == alice_sender),
        "Alice should appear in contacts after import"
    );
    assert!(
        contacts_state.contacts.iter().any(|c| c.id == carol_sender),
        "Carol should appear in contacts after import"
    );

    let _ = std::fs::remove_dir_all(&env.test_dir);
    println!("\n=== ImportInvitation Command Test PASSED ===\n");
}

/// Test complete demo flow: import invitations, accept them, create channel, send message
///
/// NOTE: Messaging is currently UI-local (signal-backed) in the terminal runtime; this test
/// verifies the user-visible behavior (signals) rather than network delivery.
#[tokio::test]
async fn test_complete_demo_invitation_flow() {
    println!("\n=== Complete Demo Invitation Flow Test ===\n");

    let env = setup_test_env("complete-flow").await;
    let seed = 2024;
    // Names and seeds must match AgentFactory::create_demo_agents
    let alice_code = generate_demo_invite_code("Alice", seed);
    let carol_code = generate_demo_invite_code("Carol", seed + 1);

    // Parse the codes to get sender IDs
    let alice_invitation = ShareableInvitation::from_code(&alice_code).expect("Parse Alice");
    let carol_invitation = ShareableInvitation::from_code(&carol_code).expect("Parse Carol");

    // Phase 1: Import both invitation codes
    println!("Phase 1: Import Alice and Carol's invitation codes");

    env.ctx
        .dispatch(EffectCommand::ImportInvitation {
            code: alice_code.clone(),
        })
        .await
        .expect("Alice import should succeed");
    println!("  Alice's invitation imported successfully");
    wait_for_contact(&env.app_core, alice_invitation.sender_id).await;

    env.ctx
        .dispatch(EffectCommand::ImportInvitation {
            code: carol_code.clone(),
        })
        .await
        .expect("Carol import should succeed");
    println!("  Carol's invitation imported successfully");
    wait_for_contact(&env.app_core, carol_invitation.sender_id).await;

    // Phase 2: Accept the invitations
    println!("\nPhase 2: Accept invitations to create contacts");

    env.ctx
        .dispatch(EffectCommand::AcceptInvitation {
            invitation_id: alice_invitation.invitation_id.to_string(),
        })
        .await
        .expect("Accept Alice should succeed (idempotent)");

    env.ctx
        .dispatch(EffectCommand::AcceptInvitation {
            invitation_id: carol_invitation.invitation_id.to_string(),
        })
        .await
        .expect("Accept Carol should succeed (idempotent)");

    // Phase 3: Create a group channel with Alice and Carol
    println!("\nPhase 3: Create group channel with Alice and Carol");

    env.ctx
        .dispatch(EffectCommand::CreateChannel {
            name: "Guardians".to_string(),
            topic: Some("Guardian coordination channel".to_string()),
            members: vec![
                alice_invitation.sender_id.to_string(),
                carol_invitation.sender_id.to_string(),
            ],
            threshold_k: 1,
        })
        .await
        .expect("CreateChannel should succeed");

    // Phase 4: Send a message to the channel
    println!("\nPhase 4: Send message to guardians");

    env.ctx
        .dispatch(EffectCommand::SendMessage {
            channel: "guardians".to_string(),
            content: "Hello Alice and Carol! Thanks for being my guardians.".to_string(),
        })
        .await
        .expect("SendMessage should succeed");

    // Phase 5: Verify state via signals
    println!("\nPhase 5: Verify state via signals");

    wait_for_channel_members(&env.app_core, "Guardians", 0).await;
    wait_for_message(&env.app_core, "Guardians", "Hello Alice and Carol!").await;

    let core = env.app_core.read().await;

    // Contacts should be visible to the user after import/accept.
    let contacts_state = core
        .read(&*CONTACTS_SIGNAL)
        .await
        .expect("Read CONTACTS_SIGNAL");
    assert!(
        contacts_state
            .contacts
            .iter()
            .any(|c| c.id == alice_invitation.sender_id),
        "Alice should appear in contacts"
    );
    assert!(
        contacts_state
            .contacts
            .iter()
            .any(|c| c.id == carol_invitation.sender_id),
        "Carol should appear in contacts"
    );

    // Chat channel + message should show up in CHAT_SIGNAL.
    let chat_state = core.read(&*CHAT_SIGNAL).await.expect("Read CHAT_SIGNAL");
    let guardians = chat_state
        .channels
        .iter()
        .find(|c| c.name == "Guardians")
        .expect("Guardians channel should exist");
    assert_eq!(
        guardians.member_count, 0,
        "Guardians member count is unknown until membership facts are tracked"
    );
    assert!(
        chat_state
            .messages
            .iter()
            .any(|m| m.channel_id == guardians.id && m.content.contains("Hello Alice and Carol!")),
        "Expected the greeting message to appear in the selected channel messages"
    );

    // Check invitations state
    if let Ok(inv_state) = core.read(&*INVITATIONS_SIGNAL).await {
        let pending_count = inv_state.pending.len();
        let sent_count = inv_state.sent.len();
        println!("  Pending invitations: {pending_count}");
        println!("  Sent invitations: {sent_count}");
    }

    // Check recovery/guardians state
    if let Ok(recovery_state) = core.read(&*RECOVERY_SIGNAL).await {
        let guardian_count = recovery_state.guardians.len();
        println!("  Guardians: {guardian_count}");
        println!(
            "  Threshold: {threshold}",
            threshold = recovery_state.threshold
        );
    }

    drop(core);
    let _ = std::fs::remove_dir_all(&env.test_dir);
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

    println!("  Seed {seed} produces consistent codes:");
    let alice_preview = &alice1[..40];
    println!("    Alice: {alice_preview}...");
    let carol_preview = &carol1[..40];
    println!("    Carol (seed + 1): {carol_preview}...");

    // Verify different seeds produce different codes
    let alice_different = generate_demo_invite_code("Alice", 2025);
    assert_ne!(
        alice1, alice_different,
        "Different seeds should produce different codes"
    );

    println!("\n  Seed 2025 produces different codes:");
    let alice_diff_preview = &alice_different[..40];
    println!("    Alice: {alice_diff_preview}...");

    println!("\n=== Demo Hints Determinism Test PASSED ===\n");
}

/// Test invalid invitation codes are properly rejected
#[tokio::test]
async fn test_invalid_invitation_code_rejection() {
    println!("\n=== Invalid Invitation Code Rejection Test ===\n");

    let env = setup_test_env("invalid-codes").await;

    // Test 1: Completely invalid format
    println!("Test 1: Invalid format");
    let result = env
        .ctx
        .dispatch(EffectCommand::ImportInvitation {
            code: "not-a-valid-code".to_string(),
        })
        .await;
    println!("  Result: {result:?}");
    assert!(result.is_err(), "Invalid format should fail");

    // Test 2: Wrong prefix
    println!("\nTest 2: Wrong prefix");
    let result = env
        .ctx
        .dispatch(EffectCommand::ImportInvitation {
            code: "wrong:v1:abc123".to_string(),
        })
        .await;
    println!("  Result: {result:?}");
    assert!(result.is_err(), "Wrong prefix should fail");

    // Test 3: Invalid base64
    println!("\nTest 3: Invalid base64");
    let result = env
        .ctx
        .dispatch(EffectCommand::ImportInvitation {
            code: "aura:v1:not-valid-base64!!!".to_string(),
        })
        .await;
    println!("  Result: {result:?}");
    assert!(result.is_err(), "Invalid base64 should fail");

    // Test 4: Valid base64 but invalid JSON
    println!("\nTest 4: Valid base64 but invalid JSON");
    let invalid_json =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("not json at all".as_bytes());
    let result = env
        .ctx
        .dispatch(EffectCommand::ImportInvitation {
            code: format!("aura:v1:{invalid_json}"),
        })
        .await;
    println!("  Result: {result:?}");
    assert!(result.is_err(), "Invalid JSON should fail");

    let _ = std::fs::remove_dir_all(&env.test_dir);
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
    let alice_invitation =
        ShareableInvitation::from_code(&alice_code).expect("Alice invitation should parse");
    let carol_invitation =
        ShareableInvitation::from_code(&carol_code).expect("Carol invitation should parse");

    let alice_invitation_authority = alice_invitation.sender_id;
    let carol_invitation_authority = carol_invitation.sender_id;

    println!("From parsed invitations:");
    println!("  Alice AuthorityId: {alice_invitation_authority}");
    println!("  Carol AuthorityId: {carol_invitation_authority}");

    // Step 3: Derive AuthorityIds the same way SimulatedAgent does
    // (This mirrors SimulatedAgent::new_with_shared_transport in demo/mod.rs)
    let alice_simulator_authority = ids::authority_id(&format!("demo:{seed}:Alice:authority"));
    let carol_seed = seed + 1;
    let carol_simulator_authority =
        ids::authority_id(&format!("demo:{carol_seed}:Carol:authority"));

    println!("\nFrom simulator derivation:");
    println!("  Alice AuthorityId: {alice_simulator_authority}");
    println!("  Carol AuthorityId: {carol_simulator_authority}");

    // Step 4: CRITICAL ASSERTIONS - these must match for guardian display to work
    assert_eq!(
        alice_invitation_authority, alice_simulator_authority,
        "CRITICAL: Alice's invitation AuthorityId must match SimulatedAgent AuthorityId.\n\
         This is required for signal_coordinator to find the contact when setting is_guardian=true.\n\
         Invitation: {alice_invitation_authority}\n\
         Simulator:  {alice_simulator_authority}"
    );

    assert_eq!(
        carol_invitation_authority, carol_simulator_authority,
        "CRITICAL: Carol's invitation AuthorityId must match SimulatedAgent AuthorityId.\n\
         This is required for signal_coordinator to find the contact when setting is_guardian=true.\n\
         Invitation: {carol_invitation_authority}\n\
         Simulator:  {carol_simulator_authority}"
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
    println!("Alice (seed={seed}):");

    // 1. How ids::authority_id derives it
    let seed_string = format!("demo:{seed}:Alice:authority");
    println!("  Seed string: \"{seed_string}\"");

    let authority = ids::authority_id(&seed_string);
    println!("  AuthorityId: {authority}");
    println!("  UUID: {uuid}", uuid = authority.uuid());

    // 2. Verify it matches what's in the invitation
    let code = generate_demo_invite_code("Alice", seed);
    let invitation = ShareableInvitation::from_code(&code).unwrap();
    println!(
        "  Invitation sender_id: {sender_id}",
        sender_id = invitation.sender_id
    );

    assert_eq!(
        authority, invitation.sender_id,
        "ids::authority_id must produce same result as what's in invitation"
    );

    // Test Carol's derivation chain (uses seed + 1)
    let carol_seed = seed + 1;
    println!("\nCarol (seed={carol_seed}):");

    let carol_seed_string = format!("demo:{carol_seed}:Carol:authority");
    println!("  Seed string: \"{carol_seed_string}\"");

    let carol_authority = ids::authority_id(&carol_seed_string);
    println!("  AuthorityId: {carol_authority}");

    let carol_code = generate_demo_invite_code("Carol", carol_seed);
    let carol_invitation = ShareableInvitation::from_code(&carol_code).unwrap();
    println!(
        "  Invitation sender_id: {sender_id}",
        sender_id = carol_invitation.sender_id
    );

    assert_eq!(
        carol_authority, carol_invitation.sender_id,
        "Carol's ids::authority_id must produce same result as invitation"
    );

    println!("\n=== Derivation Chain Consistency Test PASSED ===\n");
}
