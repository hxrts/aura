//! DkdChoreography Protocol Test
//!
//! Tests the distributed key derivation choreography harness setup.
//! Full protocol execution requires actual DKD message types.

use aura_agent::AuraProtocolAdapter;
use aura_authentication::dkd_runners::DkdChoreographyRole;
use aura_core::{AuthorityId, DeviceId};
use aura_mpst::rumpsteak_aura_choreography::RoleId;
use aura_simulator::{SimulatedMessageBus, TestEffectSystem};
use aura_testkit::ProtocolTest;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn dkd_choreography_harness_builds() {
    // Test that protocol harness can be built
    let initiator_device = DeviceId::from_uuid(Uuid::new_v4());
    let participant_device = DeviceId::from_uuid(Uuid::new_v4());

    let test = ProtocolTest::new("DkdChoreography")
        .bind_role("Initiator", initiator_device)
        .bind_role("Participant", participant_device)
        .expect_success();

    let _harness = test.build_harness().expect("protocol harness should build");
}

#[tokio::test]
async fn dkd_choreography_adapter_setup() {
    // Test that protocol adapters can be created and sessions started
    let initiator_device = DeviceId::from_uuid(Uuid::new_v4());
    let participant_device = DeviceId::from_uuid(Uuid::new_v4());

    let initiator_auth = AuthorityId::from_uuid(initiator_device.uuid());
    let participant_auth = AuthorityId::from_uuid(participant_device.uuid());

    let mut role_map = HashMap::new();
    role_map.insert(DkdChoreographyRole::Initiator, initiator_auth);
    role_map.insert(DkdChoreographyRole::Participant, participant_auth);

    let bus = Arc::new(SimulatedMessageBus::new());
    let session_id = Uuid::new_v4();

    let initiator_effects = TestEffectSystem::new(
        bus.clone(),
        initiator_device,
        DkdChoreographyRole::Initiator.role_index().unwrap_or(0),
    )
    .expect("effects");

    let participant_effects = TestEffectSystem::new(
        bus.clone(),
        participant_device,
        DkdChoreographyRole::Participant.role_index().unwrap_or(1),
    )
    .expect("effects");

    // Verify adapters can be created
    let mut initiator_adapter = AuraProtocolAdapter::new(
        Arc::new(initiator_effects),
        initiator_auth,
        DkdChoreographyRole::Initiator,
        role_map.clone(),
    );

    let mut participant_adapter = AuraProtocolAdapter::new(
        Arc::new(participant_effects),
        participant_auth,
        DkdChoreographyRole::Participant,
        role_map.clone(),
    );

    // Verify sessions can start
    initiator_adapter
        .start_session(session_id)
        .await
        .expect("initiator session start");
    participant_adapter
        .start_session(session_id)
        .await
        .expect("participant session start");

    // Verify sessions can end
    initiator_adapter
        .end_session()
        .await
        .expect("initiator session end");
    participant_adapter
        .end_session()
        .await
        .expect("participant session end");
}
