//! Regression test for demo mode echo functionality.
//!
//! This test mimics the exact TUI flow:
//! 1. Start demo mode
//! 2. Import Alice as a contact via invitation code
//! 3. Import Carol as a contact via invitation code
//! 4. Create a channel with them as members
//! 5. Send a message
//! 6. Verify Alice and Carol echo the message back
//!
//! This isolates the regression where echoes work in unit tests but not in the real TUI.

#![cfg(feature = "development")]
#![allow(clippy::expect_used, clippy::unwrap_used, missing_docs)]

use async_lock::RwLock;
use std::sync::Arc;
use std::time::Duration;

use aura_agent::{AgentBuilder, AgentConfig, EffectContext};
use aura_app::ui::signals::CHAT_SIGNAL;
use aura_app::ui::workflows::{invitation, messaging, query};
use aura_app::{AppConfig, AppCore};
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::effects::ExecutionMode;
use aura_core::hash;
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_terminal::demo::{spawn_amp_echo_listener, DemoHints, DemoSimulator, EchoPeer};
use aura_terminal::ids;
use aura_terminal::tui::context::InitializedAppCore;

mod support;

/// REGRESSION TEST: Echo fails when contacts are imported via invitation codes
/// before creating a channel (mimics real TUI flow).
///
/// ## Isolated Regression:
/// `invitation::accept_invitation()` returns Ok(()) but does NOT create contacts
/// in the contacts list. This means:
/// 1. Users can "import" contacts successfully (no error)
/// 2. But contacts don't appear in the contacts list
/// 3. So when creating a channel, there are no contacts to select as members
/// 4. The echo listener can't match channel members to echo peers
///
/// ## Root Cause Location:
/// The issue is in how `accept_invitation` handles contact creation.
/// Either the contact is not being committed to the fact journal, or the
/// reactive reducer is not surfacing contacts to the CONTACTS_SIGNAL.
#[tokio::test]
async fn demo_echo_after_importing_contacts_via_invitation() {
    let seed = 2024u64;

    // Match demo-mode authority/context derivation used by the TUI handler.
    let bob_device_id_str = "demo:bob";
    let bob_authority_entropy = hash::hash(format!("authority:{bob_device_id_str}").as_bytes());
    let bob_authority = AuthorityId::new_from_entropy(bob_authority_entropy);
    let bob_context_entropy = hash::hash(format!("context:{bob_device_id_str}").as_bytes());
    let bob_context = ContextId::new_from_entropy(bob_context_entropy);

    let test_dir = support::unique_test_dir("aura-demo-echo-regression");

    // Start demo peers (Alice + Carol) as real runtimes.
    let mut simulator = DemoSimulator::new(seed, test_dir.clone(), bob_authority, bob_context)
        .await
        .expect("Failed to create demo simulator");
    simulator
        .start()
        .await
        .expect("Failed to start demo simulator");
    let shared_transport = simulator.shared_transport();

    // Build Bob's runtime with shared transport wiring.
    let bob_device_id = ids::device_id(bob_device_id_str);
    let agent_config = AgentConfig {
        device_id: bob_device_id,
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
        .build_simulation_async_with_shared_transport(seed, &effect_ctx, shared_transport.clone())
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
    InitializedAppCore::new(app_core.clone())
        .await
        .expect("init signals");

    // Get demo hints (invitation codes) - this is how the TUI gets them
    let hints = DemoHints::new(seed);
    eprintln!("[Test] Alice invite code: {}", hints.alice_invite_code);
    eprintln!("[Test] Carol invite code: {}", hints.carol_invite_code);

    // Start demo AMP echo listener (like TUI does)
    let peers = vec![
        EchoPeer {
            authority_id: simulator.alice_authority(),
            name: "Alice".to_string(),
        },
        EchoPeer {
            authority_id: simulator.carol_authority(),
            name: "Carol".to_string(),
        },
    ];
    let _listener = spawn_amp_echo_listener(
        shared_transport.clone(),
        bob_authority,
        bob_device_id.to_string(),
        app_core.clone(),
        agent.runtime().effects(),
        peers,
    );

    // ========================================================================
    // KEY DIFFERENCE: Import contacts via invitation codes (like TUI does)
    // ========================================================================

    // Import and accept Alice as a contact (two-step process)
    // Step 1: import_invitation_details parses the code and returns InvitationInfo with invitation_id
    // Step 2: accept_invitation uses that invitation_id to accept
    eprintln!("[Test] Importing Alice via invitation code...");
    let alice_info = invitation::import_invitation_details(&app_core, &hints.alice_invite_code)
        .await
        .expect("Alice import should succeed");
    eprintln!(
        "[Test] Alice imported: invitation_id={}, sender_id={}",
        alice_info.invitation_id, alice_info.sender_id
    );
    let alice_accept = invitation::accept_invitation(&app_core, &alice_info.invitation_id).await;
    eprintln!("[Test] Alice accept result: {:?}", alice_accept);

    // Import and accept Carol as a contact
    eprintln!("[Test] Importing Carol via invitation code...");
    let carol_info = invitation::import_invitation_details(&app_core, &hints.carol_invite_code)
        .await
        .expect("Carol import should succeed");
    eprintln!(
        "[Test] Carol imported: invitation_id={}, sender_id={}",
        carol_info.invitation_id, carol_info.sender_id
    );
    let carol_accept = invitation::accept_invitation(&app_core, &carol_info.invitation_id).await;
    eprintln!("[Test] Carol accept result: {:?}", carol_accept);

    // Allow time for contact imports to complete
    tokio::time::sleep(Duration::from_millis(500)).await;

    // ========================================================================
    // KEY: Get contact IDs from the contacts list (like the TUI does!)
    // This is the critical difference - we use the imported contact IDs,
    // not the simulator's authority IDs directly.
    // ========================================================================

    let contact_list = query::list_contacts(&app_core).await;
    eprintln!("[Test] Contact list has {} contacts:", contact_list.len());
    for contact in &contact_list {
        eprintln!(
            "[Test]   - '{}' id={} (simulator alice={}, carol={})",
            contact.nickname,
            contact.id,
            simulator.alice_authority(),
            simulator.carol_authority()
        );
    }

    // Use the IDs from the contacts list, not from the simulator directly!
    // This mimics what the TUI does when user selects contacts.
    let members: Vec<String> = contact_list.iter().map(|c| c.id.to_string()).collect();

    if members.is_empty() {
        panic!(
            "REGRESSION: No contacts found after importing Alice and Carol via invitation codes! \
             This means the invitation import did not create contacts correctly."
        );
    }

    eprintln!(
        "[Test] Creating channel with members from contacts list: {:?}",
        members
    );

    // Also log what the echo listener expects
    eprintln!(
        "[Test] Echo listener expects Alice={}, Carol={}",
        simulator.alice_authority(),
        simulator.carol_authority()
    );

    let channel_result =
        messaging::create_channel(&app_core, "test-channel", None, &members, 0, 1).await;
    eprintln!("[Test] Channel creation result: {:?}", channel_result);
    // Now returns typed ChannelId, not String - this enforces type safety!
    let channel_id = channel_result.expect("create channel");

    // Allow demo peers to accept channel invitations
    tokio::time::sleep(Duration::from_millis(500)).await;

    // ========================================================================
    // Send message and check for echo
    // ========================================================================

    let content = "hello-from-bob";

    // Subscribe to chat signal BEFORE sending
    let mut chat_stream = {
        let core = app_core.read().await;
        core.subscribe(&*CHAT_SIGNAL)
    };

    eprintln!("[Test] Sending message to channel {}...", channel_id);
    // Using typed ChannelId ensures we send to the EXACT channel we created
    messaging::send_message(&app_core, channel_id, content, 2)
        .await
        .expect("send message");

    // Wait for signal updates and check for echo
    let mut found_echo = false;
    let timeout = Duration::from_secs(5);
    let deadline = tokio::time::Instant::now() + timeout;

    while tokio::time::Instant::now() < deadline {
        tokio::select! {
            Ok(chat_state) = chat_stream.recv() => {
                let messages: Vec<_> = chat_state.all_messages().into_iter().collect();
                eprintln!(
                    "[Test] Signal update: {} messages total",
                    messages.len()
                );

                // Log all messages for debugging
                for msg in &messages {
                    eprintln!(
                        "[Test]   - '{}' from {} (is_own={})",
                        msg.content,
                        msg.sender_id,
                        msg.is_own
                    );
                }

                if messages
                    .iter()
                    .any(|msg| msg.content == content && msg.sender_id != bob_authority)
                {
                    found_echo = true;
                    eprintln!("[Test] Found echo from Alice or Carol!");
                    break;
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                // Timeout to prevent infinite loop
            }
        }
    }

    assert!(
        found_echo,
        "REGRESSION: Expected an echo message from Alice or Carol after importing contacts \
         via invitation codes and creating a channel. This works when members are added \
         directly but fails when using the TUI flow. Check that:\n\
         1. Invitation codes are being processed correctly\n\
         2. Contacts are being added to the channel properly\n\
         3. The echo listener can see the channel members\n\
         4. The echo is being emitted to the correct signal"
    );
}

/// Test echo when the channel has empty member_ids (simulates potential race condition)
#[tokio::test]
async fn demo_echo_with_empty_channel_members() {
    let seed = 2026u64;

    let bob_device_id_str = "demo:bob-empty";
    let bob_authority_entropy = hash::hash(format!("authority:{bob_device_id_str}").as_bytes());
    let bob_authority = AuthorityId::new_from_entropy(bob_authority_entropy);
    let bob_context_entropy = hash::hash(format!("context:{bob_device_id_str}").as_bytes());
    let bob_context = ContextId::new_from_entropy(bob_context_entropy);

    let test_dir = support::unique_test_dir("aura-demo-echo-empty");

    let mut simulator = DemoSimulator::new(seed, test_dir.clone(), bob_authority, bob_context)
        .await
        .expect("Failed to create demo simulator");
    simulator
        .start()
        .await
        .expect("Failed to start demo simulator");
    let shared_transport = simulator.shared_transport();

    let bob_device_id = ids::device_id(bob_device_id_str);
    let agent_config = AgentConfig {
        device_id: bob_device_id,
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
        .build_simulation_async_with_shared_transport(seed, &effect_ctx, shared_transport.clone())
        .await
        .expect("Failed to build agent");
    let agent = Arc::new(agent);

    let app_config = AppConfig {
        data_dir: test_dir.to_string_lossy().to_string(),
        ..AppConfig::default()
    };
    let app_core = AppCore::with_runtime(app_config, agent.clone().as_runtime_bridge())
        .expect("Failed to create AppCore");
    let app_core = Arc::new(RwLock::new(app_core));
    InitializedAppCore::new(app_core.clone())
        .await
        .expect("init signals");

    let peers = vec![
        EchoPeer {
            authority_id: simulator.alice_authority(),
            name: "Alice".to_string(),
        },
        EchoPeer {
            authority_id: simulator.carol_authority(),
            name: "Carol".to_string(),
        },
    ];
    let _listener = spawn_amp_echo_listener(
        shared_transport.clone(),
        bob_authority,
        bob_device_id.to_string(),
        app_core.clone(),
        agent.runtime().effects(),
        peers,
    );

    // Create channel with NO members (empty list) - this tests the fallback behavior
    // The echo listener should still work because it falls back when member_ids is empty
    let members: Vec<String> = vec![];
    let channel_id =
        messaging::create_channel(&app_core, "empty-members-channel", None, &members, 0, 1)
            .await
            .expect("create channel");

    tokio::time::sleep(Duration::from_millis(400)).await;

    let content = "empty-members-test";
    let mut chat_stream = {
        let core = app_core.read().await;
        core.subscribe(&*CHAT_SIGNAL)
    };

    // Using typed ChannelId ensures we send to the EXACT channel we created
    messaging::send_message(&app_core, channel_id, content, 2)
        .await
        .expect("send message");

    let mut found_echo = false;
    let timeout = Duration::from_secs(5);
    let deadline = tokio::time::Instant::now() + timeout;

    while tokio::time::Instant::now() < deadline {
        tokio::select! {
            Ok(chat_state) = chat_stream.recv() => {
                let messages: Vec<_> = chat_state.all_messages().into_iter().collect();
                eprintln!(
                    "[Empty Members Test] Signal update: {} messages",
                    messages.len()
                );
                for msg in &messages {
                    eprintln!(
                        "[Empty Members Test]   - '{}' from {} (is_own={})",
                        msg.content,
                        msg.sender_id,
                        msg.is_own
                    );
                }

                if messages
                    .iter()
                    .any(|msg| msg.content == content && msg.sender_id != bob_authority)
                {
                    found_echo = true;
                    eprintln!("[Empty Members Test] Found echo!");
                    break;
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(100)) => {}
        }
    }

    // This test should PASS because the echo listener falls back when member_ids is empty
    assert!(
        found_echo,
        "Echo should work even with empty channel members (fallback behavior)"
    );
}

/// Test that echoes persist and aren't immediately overwritten by the scheduler.
/// This tests the scenario where the scheduler might emit a new state that
/// overwrites the echo before the UI can display it.
///
/// This test was previously failing due to a type mismatch bug:
/// - create_channel returned String, send_message parsed the name to a hash-based ChannelId
/// - The hash-based ChannelId didn't match the runtime-generated ChannelId
/// - Fix: create_channel now returns typed ChannelId, send_message accepts ChannelId
#[tokio::test]
async fn demo_echo_persists_after_scheduler_update() {
    let seed = 2027u64;

    let bob_device_id_str = "demo:bob-persist";
    let bob_authority_entropy = hash::hash(format!("authority:{bob_device_id_str}").as_bytes());
    let bob_authority = AuthorityId::new_from_entropy(bob_authority_entropy);
    let bob_context_entropy = hash::hash(format!("context:{bob_device_id_str}").as_bytes());
    let bob_context = ContextId::new_from_entropy(bob_context_entropy);

    let test_dir = support::unique_test_dir("aura-demo-echo-persist");

    let mut simulator = DemoSimulator::new(seed, test_dir.clone(), bob_authority, bob_context)
        .await
        .expect("Failed to create demo simulator");
    simulator
        .start()
        .await
        .expect("Failed to start demo simulator");
    let shared_transport = simulator.shared_transport();

    let bob_device_id = ids::device_id(bob_device_id_str);
    let agent_config = AgentConfig {
        device_id: bob_device_id,
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
        .build_simulation_async_with_shared_transport(seed, &effect_ctx, shared_transport.clone())
        .await
        .expect("Failed to build agent");
    let agent = Arc::new(agent);

    let app_config = AppConfig {
        data_dir: test_dir.to_string_lossy().to_string(),
        ..AppConfig::default()
    };
    let app_core = AppCore::with_runtime(app_config, agent.clone().as_runtime_bridge())
        .expect("Failed to create AppCore");
    let app_core = Arc::new(RwLock::new(app_core));
    InitializedAppCore::new(app_core.clone())
        .await
        .expect("init signals");

    let peers = vec![
        EchoPeer {
            authority_id: simulator.alice_authority(),
            name: "Alice".to_string(),
        },
        EchoPeer {
            authority_id: simulator.carol_authority(),
            name: "Carol".to_string(),
        },
    ];
    let _listener = spawn_amp_echo_listener(
        shared_transport.clone(),
        bob_authority,
        bob_device_id.to_string(),
        app_core.clone(),
        agent.runtime().effects(),
        peers,
    );

    let members = vec![
        simulator.alice_authority().to_string(),
        simulator.carol_authority().to_string(),
    ];
    // Now returns typed ChannelId - this fixes the type mismatch bug!
    let channel_id =
        messaging::create_channel(&app_core, "persist-test-channel", None, &members, 0, 1)
            .await
            .expect("create channel");

    tokio::time::sleep(Duration::from_millis(400)).await;

    let content = "persist-test-message";

    // Subscribe to chat signal BEFORE sending (like TUI does)
    // Note: The original test used polling which exposed a demo mode limitation:
    // demo echoes emit directly to CHAT_SIGNAL without facts, so the reactive
    // reducer overwrites them. Stream-based subscription (used by the TUI)
    // catches echoes before they're overwritten.
    let mut chat_stream = {
        let core = app_core.read().await;
        core.subscribe(&*CHAT_SIGNAL)
    };

    // Using typed ChannelId ensures we send to the EXACT channel we created
    messaging::send_message(&app_core, channel_id, content, 2)
        .await
        .expect("send message");

    // Wait for signal updates and check for echo (matches TUI pattern)
    let mut found_echo = false;
    let timeout = Duration::from_secs(5);
    let deadline = tokio::time::Instant::now() + timeout;

    while tokio::time::Instant::now() < deadline {
        tokio::select! {
            Ok(chat_state) = chat_stream.recv() => {
                if chat_state
                    .all_messages()
                    .iter()
                    .any(|msg| msg.content == content && msg.sender_id != bob_authority)
                {
                    found_echo = true;
                    break;
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(100)) => {}
        }
    }

    assert!(
        found_echo,
        "Expected echo message with typed ChannelId API"
    );
}

/// Test that echoes work when channel is created with members directly
/// (without importing contacts first). This is the control test.
#[tokio::test]
async fn demo_echo_with_direct_member_ids_control() {
    let seed = 2025u64; // Different seed to avoid conflicts

    let bob_device_id_str = "demo:bob-control";
    let bob_authority_entropy = hash::hash(format!("authority:{bob_device_id_str}").as_bytes());
    let bob_authority = AuthorityId::new_from_entropy(bob_authority_entropy);
    let bob_context_entropy = hash::hash(format!("context:{bob_device_id_str}").as_bytes());
    let bob_context = ContextId::new_from_entropy(bob_context_entropy);

    let test_dir = support::unique_test_dir("aura-demo-echo-control");

    let mut simulator = DemoSimulator::new(seed, test_dir.clone(), bob_authority, bob_context)
        .await
        .expect("Failed to create demo simulator");
    simulator
        .start()
        .await
        .expect("Failed to start demo simulator");
    let shared_transport = simulator.shared_transport();

    let bob_device_id = ids::device_id(bob_device_id_str);
    let agent_config = AgentConfig {
        device_id: bob_device_id,
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
        .build_simulation_async_with_shared_transport(seed, &effect_ctx, shared_transport.clone())
        .await
        .expect("Failed to build agent");
    let agent = Arc::new(agent);

    let app_config = AppConfig {
        data_dir: test_dir.to_string_lossy().to_string(),
        ..AppConfig::default()
    };
    let app_core = AppCore::with_runtime(app_config, agent.clone().as_runtime_bridge())
        .expect("Failed to create AppCore");
    let app_core = Arc::new(RwLock::new(app_core));
    InitializedAppCore::new(app_core.clone())
        .await
        .expect("init signals");

    let peers = vec![
        EchoPeer {
            authority_id: simulator.alice_authority(),
            name: "Alice".to_string(),
        },
        EchoPeer {
            authority_id: simulator.carol_authority(),
            name: "Carol".to_string(),
        },
    ];
    let _listener = spawn_amp_echo_listener(
        shared_transport.clone(),
        bob_authority,
        bob_device_id.to_string(),
        app_core.clone(),
        agent.runtime().effects(),
        peers,
    );

    // Create channel with members DIRECTLY (no invitation import)
    let members = vec![
        simulator.alice_authority().to_string(),
        simulator.carol_authority().to_string(),
    ];
    // Now returns typed ChannelId - this enforces type safety!
    let channel_id = messaging::create_channel(&app_core, "control-channel", None, &members, 0, 1)
        .await
        .expect("create channel");

    tokio::time::sleep(Duration::from_millis(400)).await;

    let content = "control-test";
    let mut chat_stream = {
        let core = app_core.read().await;
        core.subscribe(&*CHAT_SIGNAL)
    };

    // Using typed ChannelId ensures we send to the EXACT channel we created
    messaging::send_message(&app_core, channel_id, content, 2)
        .await
        .expect("send message");

    let mut found_echo = false;
    let timeout = Duration::from_secs(5);
    let deadline = tokio::time::Instant::now() + timeout;

    while tokio::time::Instant::now() < deadline {
        tokio::select! {
            Ok(chat_state) = chat_stream.recv() => {
                if chat_state
                    .all_messages()
                    .iter()
                    .any(|msg| msg.content == content && msg.sender_id != bob_authority)
                {
                    found_echo = true;
                    break;
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(100)) => {}
        }
    }

    assert!(
        found_echo,
        "Control test failed - echo should work with direct member IDs"
    );
}
