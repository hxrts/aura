//! RendezvousExchange Protocol Test
//!
//! Tests the rendezvous exchange choreography harness setup.
//! Full protocol execution requires actual rendezvous message types.

#![allow(clippy::expect_used, clippy::disallowed_methods)]

use aura_agent::AuraProtocolAdapter;
use aura_core::{AuthorityId, DeviceId};
use aura_mpst::rumpsteak_aura_choreography::RoleId;
use aura_rendezvous::protocol::exchange_runners::RendezvousExchangeRole;
use aura_simulator::{SimulatedMessageBus, TestEffectSystem};
use aura_testkit::ProtocolTest;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn rendezvous_exchange_harness_builds() {
    // Test that protocol harness can be built
    let initiator_device = DeviceId::from_uuid(Uuid::new_v4());
    let responder_device = DeviceId::from_uuid(Uuid::new_v4());

    let test = ProtocolTest::new("RendezvousExchange")
        .bind_role("Initiator", initiator_device)
        .bind_role("Responder", responder_device)
        .expect_success();

    let _harness = test.build_harness().expect("protocol harness should build");
}

#[tokio::test]
async fn rendezvous_exchange_adapter_setup() {
    // Test that protocol adapters can be created and sessions started
    let initiator_device = DeviceId::from_uuid(Uuid::new_v4());
    let responder_device = DeviceId::from_uuid(Uuid::new_v4());

    let initiator_auth = AuthorityId::from_uuid(initiator_device.uuid());
    let responder_auth = AuthorityId::from_uuid(responder_device.uuid());

    let mut role_map = HashMap::new();
    role_map.insert(RendezvousExchangeRole::Initiator, initiator_auth);
    role_map.insert(RendezvousExchangeRole::Responder, responder_auth);

    let bus = Arc::new(SimulatedMessageBus::new());
    let session_id = Uuid::new_v4();

    let initiator_effects = TestEffectSystem::new(
        bus.clone(),
        initiator_device,
        RendezvousExchangeRole::Initiator.role_index().unwrap_or(0),
    )
    .expect("effects");

    let responder_effects = TestEffectSystem::new(
        bus.clone(),
        responder_device,
        RendezvousExchangeRole::Responder.role_index().unwrap_or(1),
    )
    .expect("effects");

    // Verify adapters can be created
    let mut initiator_adapter = AuraProtocolAdapter::new(
        Arc::new(initiator_effects),
        initiator_auth,
        RendezvousExchangeRole::Initiator,
        role_map.clone(),
    );

    let mut responder_adapter = AuraProtocolAdapter::new(
        Arc::new(responder_effects),
        responder_auth,
        RendezvousExchangeRole::Responder,
        role_map.clone(),
    );

    // Verify sessions can start
    initiator_adapter
        .start_session(session_id)
        .await
        .expect("initiator session start");
    responder_adapter
        .start_session(session_id)
        .await
        .expect("responder session start");

    // Verify sessions can end
    initiator_adapter
        .end_session()
        .await
        .expect("initiator session end");
    responder_adapter
        .end_session()
        .await
        .expect("responder session end");
}
