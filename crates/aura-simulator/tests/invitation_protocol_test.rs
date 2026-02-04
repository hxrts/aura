//! InvitationExchange Protocol Test
//!
//! Tests the invitation exchange choreography harness setup.
//! Full protocol execution requires actual invitation message types.

#![allow(clippy::expect_used, clippy::disallowed_methods)]

use aura_agent::AuraProtocolAdapter;
use aura_core::{AuthorityId, DeviceId};
use aura_invitation::protocol::exchange_runners::InvitationExchangeRole;
use aura_mpst::rumpsteak_aura_choreography::RoleId;
use aura_simulator::{SimulatedMessageBus, TestEffectSystem};
use aura_testkit::ProtocolTest;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn invitation_exchange_harness_builds() {
    // Test that protocol harness can be built
    let sender_device = DeviceId::from_uuid(Uuid::from_bytes([1; 16]));
    let receiver_device = DeviceId::from_uuid(Uuid::from_bytes([2; 16]));

    let test = ProtocolTest::new("InvitationExchange")
        .bind_role("Sender", sender_device)
        .bind_role("Receiver", receiver_device)
        .expect_success();

    let _harness = test.build_harness().expect("protocol harness should build");
}

#[tokio::test]
async fn invitation_exchange_adapter_setup() {
    // Test that protocol adapters can be created and sessions started
    let sender_device = DeviceId::from_uuid(Uuid::from_bytes([11; 16]));
    let receiver_device = DeviceId::from_uuid(Uuid::from_bytes([12; 16]));

    let sender_auth = AuthorityId::from_uuid(sender_device.uuid());
    let receiver_auth = AuthorityId::from_uuid(receiver_device.uuid());

    let mut role_map = HashMap::new();
    role_map.insert(InvitationExchangeRole::Sender, sender_auth);
    role_map.insert(InvitationExchangeRole::Receiver, receiver_auth);

    let bus = Arc::new(SimulatedMessageBus::new());
    let session_id = Uuid::from_bytes([13; 16]);

    let sender_effects = TestEffectSystem::new(
        bus.clone(),
        sender_device,
        InvitationExchangeRole::Sender.role_index().unwrap_or(0),
    )
    .expect("effects");

    let receiver_effects = TestEffectSystem::new(
        bus.clone(),
        receiver_device,
        InvitationExchangeRole::Receiver.role_index().unwrap_or(1),
    )
    .expect("effects");

    // Verify adapters can be created
    let mut sender_adapter = AuraProtocolAdapter::new(
        Arc::new(sender_effects),
        sender_auth,
        InvitationExchangeRole::Sender,
        role_map.clone(),
    );

    let mut receiver_adapter = AuraProtocolAdapter::new(
        Arc::new(receiver_effects),
        receiver_auth,
        InvitationExchangeRole::Receiver,
        role_map.clone(),
    );

    // Verify sessions can start
    sender_adapter
        .start_session(session_id)
        .await
        .expect("sender session start");
    receiver_adapter
        .start_session(session_id)
        .await
        .expect("receiver session start");

    // Verify sessions can end
    sender_adapter
        .end_session()
        .await
        .expect("sender session end");
    receiver_adapter
        .end_session()
        .await
        .expect("receiver session end");
}
