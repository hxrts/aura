//! AuraConsensus Protocol Test
//!
//! Tests the consensus choreography harness setup.
//! Full protocol execution requires actual consensus message types.

use aura_agent::AuraProtocolAdapter;
use aura_consensus::protocol::runners::AuraConsensusRole;
use aura_core::{AuthorityId, DeviceId};
use aura_mpst::rumpsteak_aura_choreography::RoleId;
use aura_simulator::{SimulatedMessageBus, SimulatedTransport};
use aura_testkit::ProtocolTest;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn consensus_harness_builds() {
    // Test that protocol harness can be built
    let coordinator_device = DeviceId::from_uuid(Uuid::new_v4());
    let witness_a = DeviceId::from_uuid(Uuid::new_v4());
    let witness_b = DeviceId::from_uuid(Uuid::new_v4());

    let test = ProtocolTest::new("AuraConsensus")
        .bind_role("Coordinator", coordinator_device)
        .bind_roles("Witness", &[witness_a, witness_b])
        .expect_success();

    let _harness = test.build_harness().expect("protocol harness should build");
}

#[tokio::test]
async fn consensus_adapter_setup() {
    // Test that protocol adapters can be created and sessions started
    let coordinator_device = DeviceId::from_uuid(Uuid::new_v4());
    let witness_a = DeviceId::from_uuid(Uuid::new_v4());
    let witness_b = DeviceId::from_uuid(Uuid::new_v4());

    let coordinator_auth = AuthorityId::from_uuid(coordinator_device.uuid());
    let witness_auths = vec![
        AuthorityId::from_uuid(witness_a.uuid()),
        AuthorityId::from_uuid(witness_b.uuid()),
    ];

    let mut role_map = HashMap::new();
    role_map.insert(AuraConsensusRole::Coordinator, coordinator_auth);
    role_map.insert(AuraConsensusRole::Witness(0), witness_auths[0]);
    role_map.insert(AuraConsensusRole::Witness(1), witness_auths[1]);

    let witness_roles = vec![AuraConsensusRole::Witness(0), AuraConsensusRole::Witness(1)];

    let bus = Arc::new(SimulatedMessageBus::new());
    let session_id = Uuid::new_v4();

    let coordinator_transport = SimulatedTransport::new(
        bus.clone(),
        coordinator_device,
        AuraConsensusRole::Coordinator.role_index().unwrap_or(0),
    )
    .expect("coordinator transport");

    let witness_a_transport = SimulatedTransport::new(
        bus.clone(),
        witness_a,
        AuraConsensusRole::Witness(0).role_index().unwrap_or(0),
    )
    .expect("witness transport");

    // Verify adapters can be created with role family
    let mut coordinator_adapter = AuraProtocolAdapter::new(
        Arc::new(coordinator_transport),
        coordinator_auth,
        AuraConsensusRole::Coordinator,
        role_map.clone(),
    )
    .with_role_family("Witness", witness_roles.clone());

    let mut witness_adapter = AuraProtocolAdapter::new(
        Arc::new(witness_a_transport),
        witness_auths[0],
        AuraConsensusRole::Witness(0),
        role_map.clone(),
    )
    .with_role_family("Witness", witness_roles.clone());

    // Verify sessions can start
    coordinator_adapter
        .start_session(session_id)
        .await
        .expect("coordinator session start");
    witness_adapter
        .start_session(session_id)
        .await
        .expect("witness session start");

    // Verify sessions can end
    coordinator_adapter
        .end_session()
        .await
        .expect("coordinator session end");
    witness_adapter
        .end_session()
        .await
        .expect("witness session end");
}
