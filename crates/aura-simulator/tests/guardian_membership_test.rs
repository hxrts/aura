//! GuardianMembershipChange Protocol Test
//!
//! Tests the guardian membership change choreography harness setup.
//! Full protocol execution requires actual membership change message types.

use aura_agent::AuraProtocolAdapter;
use aura_core::{AuthorityId, DeviceId};
use aura_mpst::rumpsteak_aura_choreography::RoleId;
use aura_recovery::membership_runners::GuardianMembershipChangeRole;
use aura_simulator::{SimulatedMessageBus, SimulatedTransport};
use aura_testkit::ProtocolTest;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn guardian_membership_harness_builds() {
    // Test that protocol harness can be built
    let initiator_device = DeviceId::from_uuid(Uuid::new_v4());
    let guardian1_device = DeviceId::from_uuid(Uuid::new_v4());
    let guardian2_device = DeviceId::from_uuid(Uuid::new_v4());
    let guardian3_device = DeviceId::from_uuid(Uuid::new_v4());

    let test = ProtocolTest::new("GuardianMembershipChange")
        .bind_role("ChangeInitiator", initiator_device)
        .bind_role("Guardian1", guardian1_device)
        .bind_role("Guardian2", guardian2_device)
        .bind_role("Guardian3", guardian3_device)
        .expect_success();

    let _harness = test.build_harness().expect("protocol harness should build");
}

#[tokio::test]
async fn guardian_membership_adapter_setup() {
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
    role_map.insert(GuardianMembershipChangeRole::ChangeInitiator, initiator_auth);
    role_map.insert(GuardianMembershipChangeRole::Guardian1, guardian1_auth);
    role_map.insert(GuardianMembershipChangeRole::Guardian2, guardian2_auth);
    role_map.insert(GuardianMembershipChangeRole::Guardian3, guardian3_auth);

    let guardian_roles = vec![
        GuardianMembershipChangeRole::Guardian1,
        GuardianMembershipChangeRole::Guardian2,
        GuardianMembershipChangeRole::Guardian3,
    ];

    let bus = Arc::new(SimulatedMessageBus::new());
    let session_id = Uuid::new_v4();

    let initiator_transport = SimulatedTransport::new(
        bus.clone(),
        initiator_device,
        GuardianMembershipChangeRole::ChangeInitiator
            .role_index()
            .unwrap_or(0),
    )
    .expect("initiator transport");

    let guardian1_transport = SimulatedTransport::new(
        bus.clone(),
        guardian1_device,
        GuardianMembershipChangeRole::Guardian1.role_index().unwrap_or(1),
    )
    .expect("guardian1 transport");

    let guardian2_transport = SimulatedTransport::new(
        bus.clone(),
        guardian2_device,
        GuardianMembershipChangeRole::Guardian2.role_index().unwrap_or(2),
    )
    .expect("guardian2 transport");

    // Verify adapters can be created with role family
    let mut initiator_adapter = AuraProtocolAdapter::new(
        Arc::new(initiator_transport),
        initiator_auth,
        GuardianMembershipChangeRole::ChangeInitiator,
        role_map.clone(),
    )
    .with_role_family("Guardian", guardian_roles.clone());

    let mut guardian1_adapter = AuraProtocolAdapter::new(
        Arc::new(guardian1_transport),
        guardian1_auth,
        GuardianMembershipChangeRole::Guardian1,
        role_map.clone(),
    )
    .with_role_family("Guardian", guardian_roles.clone());

    let mut guardian2_adapter = AuraProtocolAdapter::new(
        Arc::new(guardian2_transport),
        guardian2_auth,
        GuardianMembershipChangeRole::Guardian2,
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
    guardian2_adapter
        .start_session(session_id)
        .await
        .expect("guardian2 session start");

    // Verify sessions can end
    initiator_adapter
        .end_session()
        .await
        .expect("initiator session end");
    guardian1_adapter
        .end_session()
        .await
        .expect("guardian1 session end");
    guardian2_adapter
        .end_session()
        .await
        .expect("guardian2 session end");
}
