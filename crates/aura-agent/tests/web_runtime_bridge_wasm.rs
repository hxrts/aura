#![cfg(all(target_arch = "wasm32", feature = "web"))]

use std::sync::Arc;

use aura_agent::AgentBuilder;
use aura_app::signal_defs::CHAT_SIGNAL;
use aura_core::effects::{NetworkCoreEffects, ReactiveEffects};
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test(async)]
async fn web_builder_bootstraps_runtime_and_signals() {
    let agent = AgentBuilder::web()
        .storage_prefix("aura_wasm_bootstrap")
        .testing_mode()
        .build()
        .await
        .expect("build web agent");

    let chat_state = agent
        .runtime()
        .effects()
        .reactive_handler()
        .read(&*CHAT_SIGNAL)
        .await
        .expect("read default chat signal");
    assert_eq!(chat_state.channel_count(), 0);
}

#[wasm_bindgen_test(async)]
async fn web_runtime_bridge_is_constructible() {
    let agent = AgentBuilder::web()
        .storage_prefix("aura_wasm_bridge")
        .testing_mode()
        .build()
        .await
        .expect("build web agent");
    let bridge = Arc::new(agent).as_runtime_bridge();
    let authority = bridge.authority_id();
    assert!(!authority.to_string().is_empty());
}

#[wasm_bindgen_test(async)]
async fn web_network_message_flow_loopback() {
    let agent = AgentBuilder::web()
        .storage_prefix("aura_wasm_network")
        .testing_mode()
        .build()
        .await
        .expect("build web agent");

    let authority = agent.authority_id().uuid();
    let payload = b"wasm-loopback".to_vec();
    let effects = agent.runtime().effects();

    effects
        .send_to_peer(authority, payload.clone())
        .await
        .expect("send loopback");

    let (peer, received) = effects.receive().await.expect("receive loopback");
    assert_eq!(peer, authority);
    assert_eq!(received, payload);
}
