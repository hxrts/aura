#![allow(missing_docs)]
use aura_anti_entropy::{AntiEntropyConfig, AntiEntropyHandler};
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::FlowCost;
use aura_guards::chain::SendGuardChain;
use aura_simulator::handlers::effect_composer::factory::create_deterministic_environment;
use aura_testkit::DeviceTestFixture;
use uuid::Uuid;

#[tokio::test]
async fn simulator_amp_guard_chain_is_deterministic() {
    let fixture = DeviceTestFixture::new(42);
    let env = create_deterministic_environment(fixture.device_id(), 123)
        .await
        .unwrap_or_else(|err| panic!("deterministic environment: {err}"));
    let effects = env.effect_system();

    let context = ContextId::new_from_entropy([1u8; 32]);
    let peer = AuthorityId::new_from_entropy([2u8; 32]);

    let guard = SendGuardChain::new("amp:send".to_string(), context, peer, FlowCost::new(1))
        .with_operation_id("amp_send_sim");
    let result = guard
        .evaluate(effects.as_ref())
        .await
        .unwrap_or_else(|err| panic!("guard eval: {err}"));

    assert!(!result.authorized);
}

#[tokio::test]
async fn simulator_anti_entropy_guard_chain_path() {
    let fixture = DeviceTestFixture::new(7);
    let env = create_deterministic_environment(fixture.device_id(), 99)
        .await
        .unwrap_or_else(|err| panic!("deterministic environment: {err}"));
    let effects = env.effect_system();

    let context = ContextId::new_from_entropy([9u8; 32]);
    let handler = AntiEntropyHandler::new(AntiEntropyConfig::default(), context);
    let peer = Uuid::from_u128(1);

    handler.add_peer(peer).await;
    let result = handler.sync_with_peer_guarded(peer, effects.as_ref()).await;
    assert!(result.is_err());
}
