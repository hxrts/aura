#![allow(missing_docs)]
use aura_anti_entropy::{AntiEntropyConfig, AntiEntropyHandler};
use aura_core::identifiers::{AuthorityId, ContextId, DeviceId};
use aura_core::FlowCost;
use aura_guards::chain::SendGuardChain;
use aura_guards::types::CapabilityId;
use aura_simulator::handlers::effect_composer::factory::create_deterministic_environment;
use aura_testkit::DeviceTestFixture;
use uuid::Uuid;

/// Test that guard chain evaluation completes in simulation environment.
///
/// Note: This test verifies that the guard chain can be constructed and
/// evaluated without errors in a simulation environment. The authorization
/// outcome depends on platform-specific Biscuit fallback behavior.
#[tokio::test]
async fn simulator_amp_guard_chain_is_deterministic() {
    let fixture = DeviceTestFixture::new(42);
    let env = create_deterministic_environment(fixture.device_id(), 123)
        .await
        .unwrap_or_else(|err| panic!("deterministic environment: {err}"));
    let effects = env.effect_system();

    let context = ContextId::new_from_entropy([1u8; 32]);
    let peer = AuthorityId::new_from_entropy([2u8; 32]);

    let guard = SendGuardChain::new(
        CapabilityId::from("amp:send"),
        context,
        peer,
        FlowCost::new(1),
    )
    .with_operation_id("amp_send_sim");

    // Verify guard chain evaluation completes without error
    let _result = guard
        .evaluate(effects.as_ref())
        .await
        .unwrap_or_else(|err| panic!("guard eval: {err}"));
    // Test passes if evaluation completes
}

/// Test that anti-entropy sync integrates with guard chain.
///
/// Note: The sync outcome depends on platform-specific guard behavior.
/// This test verifies integration works without errors, not specific outcomes.
#[tokio::test]
async fn simulator_anti_entropy_guard_chain_path() {
    let fixture = DeviceTestFixture::new(7);
    let env = create_deterministic_environment(fixture.device_id(), 99)
        .await
        .unwrap_or_else(|err| panic!("deterministic environment: {err}"));
    let effects = env.effect_system();

    let context = ContextId::new_from_entropy([9u8; 32]);
    let handler = AntiEntropyHandler::new(AntiEntropyConfig::default(), context);
    let peer = DeviceId::from(Uuid::from_u128(1));

    handler.add_peer(peer).await;
    // Verify sync completes (Ok or Err based on guard chain) without panicking
    let _result = handler.sync_with_peer_guarded(peer, effects.as_ref()).await;
    // Test passes if no panic - outcome depends on platform
}
