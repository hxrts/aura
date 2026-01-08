//! GuardianCeremony Protocol Test
//!
//! Tests the guardian ceremony choreography harness setup.
//! Full protocol execution requires actual guardian ceremony message types.

use aura_agent::AuraProtocolAdapter;
use aura_core::{AuthorityId, DeviceId};
use aura_mpst::rumpsteak_aura_choreography::RoleId;
use aura_recovery::ceremony_runners::GuardianCeremonyRole;
use aura_simulator::{SimulatedMessageBus, SimulatedTransport};
use aura_testkit::ProtocolTest;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn guardian_ceremony_harness_builds() {
    // Test that protocol harness can be built
    let initiator_device = DeviceId::from_uuid(Uuid::new_v4());
    let guardian1_device = DeviceId::from_uuid(Uuid::new_v4());
    let guardian2_device = DeviceId::from_uuid(Uuid::new_v4());
    let guardian3_device = DeviceId::from_uuid(Uuid::new_v4());

    let test = ProtocolTest::new("GuardianCeremony")
        .bind_role("Initiator", initiator_device)
        .bind_role("Guardian0", guardian1_device)
        .bind_role("Guardian1", guardian2_device)
        .bind_role("Guardian2", guardian3_device)
        .expect_success();

    let _harness = test.build_harness().expect("protocol harness should build");
}

#[tokio::test]
async fn guardian_ceremony_adapter_setup() {
    // Test that protocol adapters can be created and sessions started
    let initiator_device = DeviceId::from_uuid(Uuid::new_v4());
    let guardian1_device = DeviceId::from_uuid(Uuid::new_v4());
    let guardian2_device = DeviceId::from_uuid(Uuid::new_v4());
    let guardian3_device = DeviceId::from_uuid(Uuid::new_v4());

    let initiator_auth = AuthorityId::from_uuid(initiator_device.uuid());
    let guardian1_auth = AuthorityId::from_uuid(guardian1_device.uuid());
    let guardian2_auth = AuthorityId::from_uuid(guardian2_device.uuid());
    let guardian3_auth = AuthorityId::from_uuid(guardian3_device.uuid());

    let mut role_map = HashMap::new();
    role_map.insert(GuardianCeremonyRole::Initiator, initiator_auth);
    role_map.insert(GuardianCeremonyRole::Guardian(0), guardian1_auth);
    role_map.insert(GuardianCeremonyRole::Guardian(1), guardian2_auth);
    role_map.insert(GuardianCeremonyRole::Guardian(2), guardian3_auth);

    let guardian_roles = vec![
        GuardianCeremonyRole::Guardian(0),
        GuardianCeremonyRole::Guardian(1),
        GuardianCeremonyRole::Guardian(2),
    ];

    let bus = Arc::new(SimulatedMessageBus::new());
    let session_id = Uuid::new_v4();

    let initiator_transport = SimulatedTransport::new(
        bus.clone(),
        initiator_device,
        GuardianCeremonyRole::Initiator.role_index().unwrap_or(0),
    )
    .expect("initiator transport");

    let guardian1_transport = SimulatedTransport::new(
        bus.clone(),
        guardian1_device,
        GuardianCeremonyRole::Guardian(0).role_index().unwrap_or(1),
    )
    .expect("guardian1 transport");

    // Verify adapters can be created with role family
    let mut initiator_adapter = AuraProtocolAdapter::new(
        Arc::new(initiator_transport),
        initiator_auth,
        GuardianCeremonyRole::Initiator,
        role_map.clone(),
    )
    .with_role_family("Guardian", guardian_roles.clone());

    let mut guardian1_adapter = AuraProtocolAdapter::new(
        Arc::new(guardian1_transport),
        guardian1_auth,
        GuardianCeremonyRole::Guardian(0),
        role_map.clone(),
    )
    .with_role_family("Guardian", guardian_roles.clone());

    // Verify sessions can start
    initiator_adapter
        .start_session(session_id)
        .await
        .expect("initiator session start");
    guardian1_adapter
        .start_session(session_id)
        .await
        .expect("guardian1 session start");

    // Verify sessions can end
    initiator_adapter
        .end_session()
        .await
        .expect("initiator session end");
    guardian1_adapter
        .end_session()
        .await
        .expect("guardian1 session end");
}
