//! EpochRotationProtocol Test
//!
//! Tests the epoch rotation choreography harness setup.
//! Full protocol execution requires actual epoch rotation message types.

#![allow(clippy::expect_used, clippy::disallowed_methods)]

use aura_agent::AuraProtocolAdapter;
use aura_core::{AuthorityId, DeviceId};
use aura_mpst::rumpsteak_aura_choreography::RoleId;
use aura_simulator::{SimulatedMessageBus, TestEffectSystem};
use aura_sync::protocols::epoch_runners::EpochRotationProtocolRole;
use aura_testkit::ProtocolTest;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn epoch_rotation_harness_builds() {
    // Test that protocol harness can be built
    let coordinator_device = DeviceId::from_uuid(Uuid::new_v4());
    let participant1_device = DeviceId::from_uuid(Uuid::new_v4());
    let participant2_device = DeviceId::from_uuid(Uuid::new_v4());

    let test = ProtocolTest::new("EpochRotationProtocol")
        .bind_role("Coordinator", coordinator_device)
        .bind_roles("Participant", &[participant1_device, participant2_device])
        .expect_success();

    let _harness = test.build_harness().expect("protocol harness should build");
}

#[tokio::test]
async fn epoch_rotation_adapter_setup() {
    // Test that protocol adapters can be created and sessions started
    let coordinator_device = DeviceId::from_uuid(Uuid::new_v4());
    let participant1_device = DeviceId::from_uuid(Uuid::new_v4());
    let participant2_device = DeviceId::from_uuid(Uuid::new_v4());

    let coordinator_auth = AuthorityId::from_uuid(coordinator_device.uuid());
    let participant1_auth = AuthorityId::from_uuid(participant1_device.uuid());
    let participant2_auth = AuthorityId::from_uuid(participant2_device.uuid());

    let mut role_map = HashMap::new();
    role_map.insert(EpochRotationProtocolRole::Coordinator, coordinator_auth);
    role_map.insert(EpochRotationProtocolRole::Participant1, participant1_auth);
    role_map.insert(EpochRotationProtocolRole::Participant2, participant2_auth);

    let participant_roles = vec![
        EpochRotationProtocolRole::Participant1,
        EpochRotationProtocolRole::Participant2,
    ];

    let bus = Arc::new(SimulatedMessageBus::new());
    let session_id = Uuid::new_v4();

    let coordinator_effects = TestEffectSystem::new(
        bus.clone(),
        coordinator_device,
        EpochRotationProtocolRole::Coordinator
            .role_index()
            .unwrap_or(0),
    )
    .expect("effects");

    let participant1_effects = TestEffectSystem::new(
        bus.clone(),
        participant1_device,
        EpochRotationProtocolRole::Participant1
            .role_index()
            .unwrap_or(1),
    )
    .expect("participant1 transport");

    // Verify adapters can be created with role family
    let mut coordinator_adapter = AuraProtocolAdapter::new(
        Arc::new(coordinator_effects),
        coordinator_auth,
        EpochRotationProtocolRole::Coordinator,
        role_map.clone(),
    )
    .with_role_family("Participant", participant_roles.clone());

    let mut participant1_adapter = AuraProtocolAdapter::new(
        Arc::new(participant1_effects),
        participant1_auth,
        EpochRotationProtocolRole::Participant1,
        role_map.clone(),
    )
    .with_role_family("Participant", participant_roles.clone());

    // Verify sessions can start
    coordinator_adapter
        .start_session(session_id)
        .await
        .expect("coordinator session start");
    participant1_adapter
        .start_session(session_id)
        .await
        .expect("participant1 session start");

    // Verify sessions can end
    coordinator_adapter
        .end_session()
        .await
        .expect("coordinator session end");
    participant1_adapter
        .end_session()
        .await
        .expect("participant1 session end");
}
