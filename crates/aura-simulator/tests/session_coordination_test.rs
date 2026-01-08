//! SessionCoordination Protocol Test
//!
//! Tests the session coordination choreography harness setup.
//! Full protocol execution requires actual session coordination message types.

use aura_agent::handlers::SessionCoordinationRole;
use aura_agent::AuraProtocolAdapter;
use aura_core::{AuthorityId, DeviceId};
use aura_mpst::rumpsteak_aura_choreography::RoleId;
use aura_simulator::{SimulatedMessageBus, SimulatedTransport};
use aura_testkit::ProtocolTest;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn session_coordination_harness_builds() {
    // Test that protocol harness can be built
    let initiator_device = DeviceId::from_uuid(Uuid::new_v4());
    let coordinator_device = DeviceId::from_uuid(Uuid::new_v4());
    let participant1_device = DeviceId::from_uuid(Uuid::new_v4());
    let participant2_device = DeviceId::from_uuid(Uuid::new_v4());

    let test = ProtocolTest::new("SessionCoordination")
        .bind_role("Initiator", initiator_device)
        .bind_role("Coordinator", coordinator_device)
        .bind_role("Participant0", participant1_device)
        .bind_role("Participant1", participant2_device)
        .expect_success();

    let _harness = test.build_harness().expect("protocol harness should build");
}

#[tokio::test]
async fn session_coordination_adapter_setup() {
    // Test that protocol adapters can be created and sessions started
    let initiator_device = DeviceId::from_uuid(Uuid::new_v4());
    let coordinator_device = DeviceId::from_uuid(Uuid::new_v4());
    let participant1_device = DeviceId::from_uuid(Uuid::new_v4());
    let participant2_device = DeviceId::from_uuid(Uuid::new_v4());

    let initiator_auth = AuthorityId::from_uuid(initiator_device.uuid());
    let coordinator_auth = AuthorityId::from_uuid(coordinator_device.uuid());
    let participant1_auth = AuthorityId::from_uuid(participant1_device.uuid());
    let participant2_auth = AuthorityId::from_uuid(participant2_device.uuid());

    let mut role_map = HashMap::new();
    role_map.insert(SessionCoordinationRole::Initiator, initiator_auth);
    role_map.insert(SessionCoordinationRole::Coordinator, coordinator_auth);
    role_map.insert(SessionCoordinationRole::Participants(0), participant1_auth);
    role_map.insert(SessionCoordinationRole::Participants(1), participant2_auth);

    let participant_roles = vec![
        SessionCoordinationRole::Participants(0),
        SessionCoordinationRole::Participants(1),
    ];

    let bus = Arc::new(SimulatedMessageBus::new());
    let session_id = Uuid::new_v4();

    let initiator_transport = SimulatedTransport::new(
        bus.clone(),
        initiator_device,
        SessionCoordinationRole::Initiator.role_index().unwrap_or(0),
    )
    .expect("initiator transport");

    let coordinator_transport = SimulatedTransport::new(
        bus.clone(),
        coordinator_device,
        SessionCoordinationRole::Coordinator.role_index().unwrap_or(1),
    )
    .expect("coordinator transport");

    let participant1_transport = SimulatedTransport::new(
        bus.clone(),
        participant1_device,
        SessionCoordinationRole::Participants(0)
            .role_index()
            .unwrap_or(2),
    )
    .expect("participant1 transport");

    // Verify adapters can be created with role family
    let mut initiator_adapter = AuraProtocolAdapter::new(
        Arc::new(initiator_transport),
        initiator_auth,
        SessionCoordinationRole::Initiator,
        role_map.clone(),
    )
    .with_role_family("Participants", participant_roles.clone());

    let mut coordinator_adapter = AuraProtocolAdapter::new(
        Arc::new(coordinator_transport),
        coordinator_auth,
        SessionCoordinationRole::Coordinator,
        role_map.clone(),
    )
    .with_role_family("Participants", participant_roles.clone());

    let mut participant1_adapter = AuraProtocolAdapter::new(
        Arc::new(participant1_transport),
        participant1_auth,
        SessionCoordinationRole::Participants(0),
        role_map.clone(),
    )
    .with_role_family("Participants", participant_roles.clone());

    // Verify sessions can start
    initiator_adapter
        .start_session(session_id)
        .await
        .expect("initiator session start");
    coordinator_adapter
        .start_session(session_id)
        .await
        .expect("coordinator session start");
    participant1_adapter
        .start_session(session_id)
        .await
        .expect("participant1 session start");

    // Verify sessions can end
    initiator_adapter
        .end_session()
        .await
        .expect("initiator session end");
    coordinator_adapter
        .end_session()
        .await
        .expect("coordinator session end");
    participant1_adapter
        .end_session()
        .await
        .expect("participant1 session end");
}
