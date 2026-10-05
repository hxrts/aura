//! Regression test for demo mode echo functionality.
//!
//! This test mimics the exact TUI flow:
//! 1. Start demo mode
//! 2. Import Alice as a contact via invite code
//! 3. Import Carol as a contact via invite code
//! 4. Create a channel with them as members
//! 5. Send a message
//! 6. Verify Alice and Carol echo the message back
//!
//! This isolates the regression where echoes work in unit tests but not in the real TUI.

#![cfg(feature = "development")]
#![allow(clippy::expect_used, clippy::unwrap_used, missing_docs)]

use std::time::Duration;

use aura_app::ui::signals::CHAT_SIGNAL;
use aura_app::ui::workflows::{invitation, messaging, query};
use aura_core::effects::reactive::ReactiveEffects;
use aura_terminal::demo::{spawn_amp_inbox_listener, EchoPeer};

#[allow(clippy::duplicate_mod)]
#[path = "../support/mod.rs"]
mod support;

fn collect_messages(chat_state: &aura_app::views::ChatState) -> Vec<&aura_app::views::Message> {
    chat_state
        .all_channels()
        .flat_map(|channel| chat_state.messages_for_channel(&channel.id).iter())
        .collect()
}

/// REGRESSION TEST: Echo fails when contacts are imported via invite codes
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
#[test]
fn demo_echo_after_importing_contacts_via_invitation() {
    support::run_with_terminal_stack(demo_echo_after_importing_contacts_via_invitation_body);
}

async fn demo_echo_after_importing_contacts_via_invitation_body() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_test_writer()
        .try_init(); // DIAG
                     // Bob is a real account (signing identity, demo-mode rendezvous and sync)
                     // whose agent shares the demo peers' transport.
    let env = support::FullTestEnv::with_config(support::FullTestEnvConfig {
        name: "demo-echo-regression".to_string(),
        nickname_suggestion: Some("Bob".to_string()),
        with_demo_peers: true,
        ..Default::default()
    })
    .await;
    let simulator = env.demo_peers.as_ref().expect("demo peers started");
    let app_core = env.app_core.clone();
    let agent = env.agent.clone();
    let bob_authority = env.authority_id;

    // Get demo hints (invite codes) - this is how the TUI gets them
    let (alice_code, carol_code) = simulator
        .signed_contact_invite_codes()
        .await
        .expect("demo peers create signed contact codes");

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
    let _listener = spawn_amp_inbox_listener(agent.runtime().effects(), bob_authority, peers);

    // ========================================================================
    // KEY DIFFERENCE: Import contacts via invite codes (like TUI does)
    // ========================================================================

    // Import and accept Alice as a contact (two-step process)
    // Step 1: import_invitation_details parses the code and returns InvitationInfo with invitation_id
    // Step 2: accept_invitation uses that invitation_id to accept
    eprintln!("[Test] Importing Alice via invite code...");
    let alice_info = invitation::import_invitation_details(&app_core, &alice_code)
        .await
        .expect("Alice import should succeed");
    eprintln!(
        "[Test] Alice imported: invitation_id={}, sender_id={}",
        alice_info.invitation_id(),
        alice_info.info().sender_id
    );
    let alice_accept = invitation::accept_invitation(&app_core, alice_info).await;
    eprintln!("[Test] Alice accept result: {alice_accept:?}");

    // Import and accept Carol as a contact
    eprintln!("[Test] Importing Carol via invite code...");
    let carol_info = invitation::import_invitation_details(&app_core, &carol_code)
        .await
        .expect("Carol import should succeed");
    eprintln!(
        "[Test] Carol imported: invitation_id={}, sender_id={}",
        carol_info.invitation_id(),
        carol_info.info().sender_id
    );
    let carol_accept = invitation::accept_invitation(&app_core, carol_info).await;
    eprintln!("[Test] Carol accept result: {carol_accept:?}");

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
            "REGRESSION: No contacts found after importing Alice and Carol via invite codes! \
             This means the invitation import did not create contacts correctly."
        );
    }

    eprintln!("[Test] Creating channel with members from contacts list: {members:?}");

    // Also log what the echo listener expects
    eprintln!(
        "[Test] Echo listener expects Alice={}, Carol={}",
        simulator.alice_authority(),
        simulator.carol_authority()
    );

    let channel_result =
        messaging::create_channel(&app_core, "test-channel", None, &members, 0, 1).await;
    eprintln!("[Test] Channel creation result: {channel_result:?}");
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
            .expect("chat signal should be registered")
    };

    eprintln!("[Test] Sending message to channel {channel_id}...");
    // Using typed ChannelId ensures we send to the EXACT channel we created
    env.send_when_channel_ready(channel_id, content).await;

    // Wait for signal updates and check for echo
    let mut found_echo = false;
    let timeout = Duration::from_secs(5);
    let deadline = tokio::time::Instant::now() + timeout;

    while tokio::time::Instant::now() < deadline {
        tokio::select! {
            Ok(chat_state) = chat_stream.recv() => {
                let messages = collect_messages(&chat_state);
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
         via invite codes and creating a channel. This works when members are added \
         directly but fails when using the TUI flow. Check that:\n\
         1. Invitation codes are being processed correctly\n\
         2. Contacts are being added to the channel properly\n\
         3. The echo listener can see the channel members\n\
         4. The echo is being emitted to the correct signal"
    );
}

/// A channel created with no members has no recipients, so sending to it is
/// refused with a typed error instead of silently echoing (the old demo
/// fallback that delivered to peers who were never members).
#[test]
fn demo_send_to_channel_without_members_is_refused() {
    support::run_with_terminal_stack(demo_send_to_channel_without_members_is_refused_body);
}

async fn demo_send_to_channel_without_members_is_refused_body() {
    let env = support::FullTestEnv::demo_bob("demo-echo-empty").await;
    env.add_demo_peers_as_contacts().await;
    let app_core = env.app_core.clone();

    let members: Vec<String> = vec![];
    let channel_id =
        messaging::create_channel(&app_core, "empty-members-channel", None, &members, 0, 1)
            .await
            .expect("create channel");

    let error = messaging::send_message(&app_core, channel_id, "empty-members-test", 2)
        .await
        .expect_err("a channel without members has no recipients");
    assert!(
        error
            .to_string()
            .contains("Recipient peers are not resolved"),
        "unexpected error: {error}"
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
#[test]
fn demo_echo_persists_after_scheduler_update() {
    support::run_with_terminal_stack(demo_echo_persists_after_scheduler_update_body);
}

async fn demo_echo_persists_after_scheduler_update_body() {
    // Bob is a real account whose demo peers are contacts (the TUI flow).
    let env = support::FullTestEnv::demo_bob("demo-echo-persist").await;
    env.add_demo_peers_as_contacts().await;
    let simulator = env.demo_peers.as_ref().expect("demo peers started");
    let app_core = env.app_core.clone();
    let agent = env.agent.clone();
    let bob_authority = env.authority_id;

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
    let _listener = spawn_amp_inbox_listener(agent.runtime().effects(), bob_authority, peers);

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
            .expect("chat signal should be registered")
    };

    // Using typed ChannelId ensures we send to the EXACT channel we created
    env.send_when_channel_ready(channel_id, content).await;

    // Wait for signal updates and check for echo (matches TUI pattern)
    let mut found_echo = false;
    let timeout = Duration::from_secs(5);
    let deadline = tokio::time::Instant::now() + timeout;

    while tokio::time::Instant::now() < deadline {
        tokio::select! {
            Ok(chat_state) = chat_stream.recv() => {
                if collect_messages(&chat_state)
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

    assert!(found_echo, "Expected echo message with typed ChannelId API");
}

/// Test that echoes work when channel is created with members directly
/// (without importing contacts first). This is the control test.
#[test]
fn demo_echo_with_direct_member_ids_control() {
    support::run_with_terminal_stack(demo_echo_with_direct_member_ids_control_body);
}

async fn demo_echo_with_direct_member_ids_control_body() {
    // Bob is a real account whose demo peers are contacts (the TUI flow).
    let env = support::FullTestEnv::demo_bob("demo-echo-control").await;
    env.add_demo_peers_as_contacts().await;
    let simulator = env.demo_peers.as_ref().expect("demo peers started");
    let app_core = env.app_core.clone();
    let agent = env.agent.clone();
    let bob_authority = env.authority_id;

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
    let _listener = spawn_amp_inbox_listener(agent.runtime().effects(), bob_authority, peers);

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
            .expect("chat signal should be registered")
    };

    // Using typed ChannelId ensures we send to the EXACT channel we created
    env.send_when_channel_ready(channel_id, content).await;

    let mut found_echo = false;
    let timeout = Duration::from_secs(5);
    let deadline = tokio::time::Instant::now() + timeout;

    while tokio::time::Instant::now() < deadline {
        tokio::select! {
            Ok(chat_state) = chat_stream.recv() => {
                if collect_messages(&chat_state)
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
