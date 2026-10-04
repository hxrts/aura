#![cfg(feature = "development")]
#![allow(clippy::expect_used, clippy::unwrap_used)]
//! Demo-mode AMP channel echo regression test.
//!
//! In demo mode, when Bob sends a message to a channel that includes Alice/Carol,
//! their demo agents should auto-echo the same message back in the same channel.

use std::time::Duration;

use aura_app::ui::signals::CHAT_SIGNAL;
use aura_app::ui::workflows::messaging;
use aura_core::effects::reactive::ReactiveEffects;
use aura_terminal::demo::{spawn_amp_inbox_listener, EchoPeer};

#[allow(clippy::duplicate_mod)]
#[path = "../support/mod.rs"]
mod support;

#[test]
fn demo_amp_channel_echoes_peer_message() {
    support::run_with_terminal_stack(demo_amp_channel_echoes_peer_message_body);
}

async fn demo_amp_channel_echoes_peer_message_body() {
    // Bob is a real account whose demo peers are contacts (the TUI flow).
    let env = support::FullTestEnv::demo_bob("demo-amp-echo").await;
    env.add_demo_peers_as_contacts().await;
    let simulator = env.demo_peers.as_ref().expect("demo peers started");
    let app_core = env.app_core.clone();
    let agent = env.agent.clone();
    let bob_authority = env.authority_id;

    // Start demo AMP echo listener to surface peer auto-replies in chat state.
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

    // Create a channel with Alice + Carol as members.
    let members = vec![
        simulator.alice_authority().to_string(),
        simulator.carol_authority().to_string(),
    ];
    // Now returns typed ChannelId - this enforces type safety!
    let channel_id = messaging::create_channel(&app_core, "guardians", None, &members, 0, 1)
        .await
        .expect("create channel");

    // Allow demo peers to accept invitations and join before sending.
    tokio::time::sleep(Duration::from_millis(400)).await;

    let content = "echo-test";

    // Subscribe to chat signal BEFORE sending to catch all updates
    let mut chat_stream = {
        let core = app_core.read().await;
        core.subscribe(&*CHAT_SIGNAL)
            .expect("chat signal should be registered")
    };

    // Using typed ChannelId ensures we send to the EXACT channel we created
    env.send_when_channel_ready(channel_id, content).await;

    // Wait for signal updates and check each one for the echoes from both peers
    let mut echoes = std::collections::HashSet::new();
    let timeout = Duration::from_secs(5);
    let deadline = tokio::time::Instant::now() + timeout;

    while tokio::time::Instant::now() < deadline {
        tokio::select! {
            Ok(chat_state) = chat_stream.recv() => {
                for channel in chat_state.all_channels() {
                    for msg in chat_state.messages_for_channel(&channel.id) {
                        if msg.content == content && msg.sender_id != bob_authority {
                            echoes.insert(msg.sender_id);
                        }
                    }
                }
                if echoes.len() >= 2 {
                    break;
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                // Just a timeout to prevent infinite loop
            }
        }
    }

    let mut missing = Vec::new();
    if !echoes.contains(&simulator.alice_authority()) {
        missing.push("Alice");
    }
    if !echoes.contains(&simulator.carol_authority()) {
        missing.push("Carol");
    }

    assert!(
        missing.is_empty(),
        "Expected echo messages from both Alice and Carol, missing: {missing:?} within {timeout:?}"
    );
}
