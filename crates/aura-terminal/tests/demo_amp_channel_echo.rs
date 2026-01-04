#![allow(clippy::expect_used, clippy::unwrap_used)]
//! Demo-mode AMP channel echo regression test.
//!
//! In demo mode, when Bob sends a message to a channel that includes Alice/Carol,
//! their demo agents should auto-echo the same message back in the same channel.

use async_lock::RwLock;
use std::sync::Arc;
use std::time::Duration;

use aura_agent::{AgentBuilder, AgentConfig, EffectContext};
use aura_app::ui::workflows::messaging;
use aura_app::{AppConfig, AppCore};
use aura_core::effects::ExecutionMode;
use aura_core::hash;
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_terminal::demo::{spawn_amp_echo_listener, DemoSimulator};
use aura_terminal::tui::context::InitializedAppCore;
use aura_terminal::ids;
use support::signals::wait_for_chat_extended;

mod support;

#[tokio::test]
async fn demo_amp_channel_echoes_peer_message() {
    let seed = 2024u64;

    // Match demo-mode authority/context derivation used by the TUI handler.
    let bob_device_id_str = "demo:bob";
    let bob_authority_entropy = hash::hash(format!("authority:{bob_device_id_str}").as_bytes());
    let bob_authority = AuthorityId::new_from_entropy(bob_authority_entropy);
    let bob_context_entropy = hash::hash(format!("context:{bob_device_id_str}").as_bytes());
    let bob_context = ContextId::new_from_entropy(bob_context_entropy);

    let test_dir = support::unique_test_dir("aura-demo-amp-echo");

    // Start demo peers (Alice + Carol) as real runtimes and share transport with Bob.
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
    let effect_ctx =
        EffectContext::new(bob_authority, bob_context, ExecutionMode::Simulation { seed });
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

    // Start demo AMP echo listener to surface peer auto-replies in chat state.
    let _listener = spawn_amp_echo_listener(
        shared_transport.clone(),
        bob_authority,
        bob_device_id.to_string(),
        app_core.clone(),
        agent.runtime().effects(),
    );

    // Create a channel with Alice + Carol as members.
    let members = vec![
        simulator.alice_authority().to_string(),
        simulator.carol_authority().to_string(),
    ];
    let _channel_id = messaging::create_channel(
        &app_core,
        "guardians",
        None,
        &members,
        0,
        1,
    )
    .await
    .expect("create channel");

    // Allow demo peers to accept invitations and join before sending.
    tokio::time::sleep(Duration::from_millis(400)).await;

    let content = "echo-test";
    messaging::send_message(&app_core, "guardians", content, 2)
        .await
        .expect("send message");

    // Expect an auto-echo from Alice or Carol in the same channel.
    wait_for_chat_extended(&app_core, |state| {
        state
            .all_messages()
            .iter()
            .any(|msg| msg.content == content && msg.sender_id != bob_authority)
    })
    .await;
}
